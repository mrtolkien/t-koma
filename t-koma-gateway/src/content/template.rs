use std::collections::HashMap;

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
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let (prefix, after_start) = rest.split_at(start);
        out.push_str(prefix);
        let Some(end) = after_start.find("}}") else {
            return Err(ContentError::TemplateParse("Unclosed {{ in template".to_string()));
        };
        let var = &after_start[2..end].trim();
        if var.is_empty() {
            return Err(ContentError::TemplateParse("Empty {{}} in template".to_string()));
        }
        let value = vars
            .get(*var)
            .ok_or_else(|| ContentError::MissingVar(var.to_string()))?;
        out.push_str(value);
        rest = &after_start[end + 2..];
    }

    out.push_str(rest);
    Ok(out)
}
