use std::collections::HashMap;

use super::ContentError;

pub type TemplateVars = HashMap<String, String>;

/// A function that resolves an include path to its file content.
pub type IncludeReader = dyn Fn(&str) -> Result<String, ContentError>;

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
    read_include: &IncludeReader,
) -> Result<String, ContentError> {
    render_template_inner(template, vars, Some(read_include), 0)
}

fn render_template_inner(
    template: &str,
    vars: &TemplateVars,
    read_include: Option<&IncludeReader>,
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
            return Err(ContentError::TemplateParse(
                "Unclosed {{ in template".to_string(),
            ));
        };
        let var = after_start[2..end].trim();
        if var.is_empty() {
            return Err(ContentError::TemplateParse(
                "Empty {{}} in template".to_string(),
            ));
        }
        if let Some(path) = parse_include(var) {
            let reader = read_include.ok_or_else(|| {
                ContentError::TemplateParse("Include is not allowed here".to_string())
            })?;
            let raw = reader(&path)?;
            let body = strip_front_matter(&raw);
            let rendered = render_template_inner(&body, vars, read_include, depth + 1)?;
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

/// Strip TOML (`+++`) or YAML (`---`) front matter from included content.
///
/// A lone `---` at the top of a file that is actually a horizontal rule (no
/// matching closing `---`) is left untouched because the function requires a
/// closing delimiter to strip anything.
fn strip_front_matter(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let delimiter = if trimmed.starts_with("+++") {
        "+++"
    } else if trimmed.starts_with("---") {
        "---"
    } else {
        return raw.to_string();
    };

    let mut lines = trimmed.lines();
    lines.next(); // skip opening delimiter
    let mut found = false;
    let mut body = Vec::new();
    for line in lines {
        if !found && line.trim() == delimiter {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_toml_front_matter() {
        let input = "+++\nid = \"test\"\n+++\n# Body";
        assert_eq!(strip_front_matter(input), "# Body");
    }

    #[test]
    fn strip_yaml_front_matter() {
        let input = "---\nname: test\ndescription: A test.\n---\n\n# Body content";
        assert_eq!(strip_front_matter(input), "\n# Body content");
    }

    #[test]
    fn no_front_matter_passthrough() {
        let input = "# Just a heading\n\nSome text.";
        assert_eq!(strip_front_matter(input), input);
    }

    #[test]
    fn lone_yaml_delimiter_not_stripped() {
        // A single --- with no closing delimiter is a horizontal rule, not front matter
        let input = "---\nSome text with no closing delimiter.";
        assert_eq!(strip_front_matter(input), input);
    }

    #[test]
    fn render_simple_variable() {
        let vars = vars_from_pairs(&[("name", "Alice")]);
        let result = render_template("Hello {{ name }}!", &vars).unwrap();
        assert_eq!(result, "Hello Alice!");
    }

    #[test]
    fn render_missing_variable_errors() {
        let vars = TemplateVars::new();
        let result = render_template("{{ missing }}", &vars);
        assert!(result.is_err());
    }
}
