use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

use super::ContentError;

pub type TemplateVars = HashMap<String, String>;

pub fn vars_from_pairs(pairs: &[(&str, &str)]) -> TemplateVars {
    let mut vars = HashMap::with_capacity(pairs.len());
    for (key, value) in pairs {
        vars.insert((*key).to_string(), (*value).to_string());
    }
    vars
}

pub fn render_template(template: &str, vars: &TemplateVars) -> Result<String, ContentError> {
    render_template_inner(template, vars, None, 0)
}

pub fn render_template_with_includes(
    template: &str,
    vars: &TemplateVars,
    source_path: &Path,
) -> Result<String, ContentError> {
    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    render_template_inner(template, vars, Some(base_dir), 0)
}

fn render_template_inner(
    template: &str,
    vars: &TemplateVars,
    base_dir: Option<&Path>,
    depth: usize,
) -> Result<String, ContentError> {
    if depth > 8 {
        return Err(ContentError::TemplateParse(
            "Template include depth exceeded".to_string(),
        ));
    }

    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let (prefix, after_start) = rest.split_at(start);
        out.push_str(prefix);
        let Some(end) = after_start.find("}}") else {
            return Err(ContentError::TemplateParse("Unclosed {{ in template".to_string()));
        };
        let var = after_start[2..end].trim();
        if var.is_empty() {
            return Err(ContentError::TemplateParse("Empty {{}} in template".to_string()));
        }
        if let Some(path) = parse_include(var) {
            let base = base_dir.ok_or_else(|| {
                ContentError::TemplateParse("Include is not allowed here".to_string())
            })?;
            let resolved = resolve_include_path(base, &path);
            let raw = fs::read_to_string(&resolved)
                .map_err(|e| ContentError::Io(resolved.clone(), e))?;
            let body = strip_front_matter(&raw);
            let rendered = render_template_inner(
                &body,
                vars,
                resolved.parent(),
                depth + 1,
            )?;
            out.push_str(&rendered);
        } else {
            let value = vars
                .get(var)
                .ok_or_else(|| ContentError::MissingVar(var.to_string()))?;
            out.push_str(value);
        }
        rest = &after_start[end + 2..];
    }

    out.push_str(rest);
    Ok(out)
}

fn parse_include(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if !trimmed.starts_with("include ") {
        return None;
    }
    let rest = trimmed.strip_prefix("include ")?;
    let rest = rest.trim();
    if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
        return Some(rest[1..rest.len() - 1].to_string());
    }
    if rest.starts_with('\'') && rest.ends_with('\'') && rest.len() >= 2 {
        return Some(rest[1..rest.len() - 1].to_string());
    }
    None
}

fn resolve_include_path(base: &Path, include: &str) -> PathBuf {
    let path = PathBuf::from(include);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn strip_front_matter(raw: &str) -> String {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("+++") {
        return raw.to_string();
    }
    let mut lines = trimmed.lines();
    lines.next();
    let mut found = false;
    let mut body = Vec::new();
    for line in lines {
        if !found && line.trim() == "+++" {
            found = true;
            continue;
        }
        if found {
            body.push(line);
        }
    }
    if found {
        body.join("\n")
    } else {
        raw.to_string()
    }
}
