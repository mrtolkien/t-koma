use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PromptFrontMatter {
    id: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    vars: Option<Vec<String>>,
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let messages_dir = manifest_dir.join("messages");
    let prompts_dir = manifest_dir.join("prompts");

    println!("cargo:rerun-if-changed={}", messages_dir.display());
    println!("cargo:rerun-if-changed={}", prompts_dir.display());

    validate_messages(&messages_dir);
    validate_prompts(&prompts_dir);
}

fn validate_messages(dir: &Path) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let entries = fs::read_dir(&path).expect("messages dir");
        for entry in entries {
            let entry = entry.expect("message entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());

            let raw_text = fs::read_to_string(&path).expect("read message file");
            let doc: toml::Value = toml::from_str(&raw_text).expect("parse message toml");
            let table = doc.as_table().expect("message file must be table");

            for (id, value) in table {
                let entry_table = value.as_table().expect("message entry must be table");
                let entry_table = toml::Value::Table(entry_table.clone());

                let scope = entry_table
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("shared");
                let suffix = None; // filename-level suffixing (future) handled by filename parsing

                validate_scope(scope, suffix, &path, id);

                let vars = entry_table
                    .get("vars")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                validate_vars(&vars, &path, id);

                let mut used = HashSet::new();
                collect_vars_from_toml(&entry_table, &mut used);
                validate_var_usage(&vars, &used, &path, id);
            }
        }
    }
}

fn validate_prompts(dir: &Path) {
    for entry in fs::read_dir(dir).expect("prompts dir") {
        let entry = entry.expect("prompt entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("file stem");
        let (id_from_name, suffix) = parse_filename(stem);

        let raw_text = fs::read_to_string(&path).expect("read prompt file");
        let (front, body) = split_front_matter(&raw_text);
        let front: PromptFrontMatter = toml::from_str(&front).expect("parse prompt front matter");

        if front.id != id_from_name {
            panic!("Prompt id '{}' does not match filename '{}'", front.id, id_from_name);
        }

        let scope = front.scope.as_deref().unwrap_or("shared");
        validate_scope(scope, suffix.as_deref(), &path, &front.id);

        let vars = front.vars.unwrap_or_default();
        validate_vars(&vars, &path, &front.id);

        let used: HashSet<String> = extract_template_vars(&body).into_iter().collect();
        validate_var_usage(&vars, &used, &path, &front.id);
    }
}

fn parse_filename(stem: &str) -> (String, Option<String>) {
    let mut parts = stem.split('@');
    let id = parts.next().unwrap_or("").to_string();
    let suffix = parts.next().map(|s| s.to_string());
    if parts.next().is_some() {
        panic!("Multiple @ in filename: {}", stem);
    }
    (id, suffix)
}

fn validate_scope(scope: &str, suffix: Option<&str>, path: &Path, id: &str) {
    match (scope, suffix) {
        ("shared", None) => {}
        (scope, Some(suffix)) if scope == format!("interface:{}", suffix) => {}
        (scope, Some(suffix)) if scope == format!("provider:{}", suffix) => {}
        ("shared", Some(_)) => panic!(
            "Shared scope requires no suffix: {} ({})",
            path.display(),
            id
        ),
        _ => panic!(
            "Scope does not match filename suffix: {} ({})",
            path.display(),
            id
        ),
    }
}

fn validate_vars(vars: &[String], path: &Path, id: &str) {
    for var in vars {
        if !is_valid_var_name(var) {
            panic!("Invalid var name '{}' in {} ({})", var, path.display(), id);
        }
    }
}

fn validate_var_usage(vars: &[String], used: &HashSet<String>, path: &Path, id: &str) {
    let declared: HashSet<String> = vars.iter().cloned().collect();

    for var in used {
        if !declared.contains(var) {
            panic!(
                "Template var '{}' not declared in vars for {} ({})",
                var,
                path.display(),
                id
            );
        }
    }

    for var in declared {
        if !used.contains(&var) {
            panic!(
                "Declared var '{}' unused in template for {} ({})",
                var,
                path.display(),
                id
            );
        }
    }
}

fn collect_vars_from_toml(value: &toml::Value, used: &mut HashSet<String>) {
    match value {
        toml::Value::String(s) => {
            for var in extract_template_vars(s) {
                used.insert(var);
            }
        }
        toml::Value::Array(arr) => {
            for item in arr {
                collect_vars_from_toml(item, used);
            }
        }
        toml::Value::Table(map) => {
            for value in map.values() {
                collect_vars_from_toml(value, used);
            }
        }
        _ => {}
    }
}

fn extract_template_vars(template: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        let var = after_start[..end].trim();
        if !var.is_empty() && !var.trim().starts_with("include ") {
            vars.push(var.to_string());
        }
        rest = &after_start[end + 2..];
    }

    vars
}

fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false; };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn split_front_matter(text: &str) -> (String, String) {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first.trim() != "+++" {
        panic!("Front matter must start with +++");
    }

    let mut front = Vec::new();
    for line in &mut lines {
        if line.trim() == "+++" {
            let body = lines.collect::<Vec<_>>().join("\n");
            return (front.join("\n"), body);
        }
        front.push(line);
    }

    panic!("Front matter must end with +++");
}
