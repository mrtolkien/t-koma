mod message;
mod prompt;
mod registry;
mod template;
pub mod ids;

use std::sync::OnceLock;

use thiserror::Error;

pub use message::{MessageContent, MessageTemplate};
pub use prompt::{PromptFrontMatter, PromptTemplate};
pub use registry::ContentRegistry;
pub use template::{render_template, vars_from_pairs, TemplateVars};

#[derive(Debug, Clone)]
pub enum ContentScope {
    Shared,
    Interface(String),
    Provider(String),
}

impl ContentScope {
    pub fn parse(value: &str) -> Result<Self, ContentError> {
        match value {
            "shared" => Ok(Self::Shared),
            _ => {
                if let Some(rest) = value.strip_prefix("interface:") {
                    return Ok(Self::Interface(rest.to_string()));
                }
                if let Some(rest) = value.strip_prefix("provider:") {
                    return Ok(Self::Provider(rest.to_string()));
                }
                Err(ContentError::Parse(format!(
                    "Invalid scope: {}",
                    value
                )))
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("content parse error: {0}")]
    Parse(String),
    #[error("content io error for {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("missing message: {0}")]
    MissingMessage(String),
    #[error("missing prompt: {0}")]
    MissingPrompt(String),
    #[error("template error: {0}")]
    TemplateParse(String),
    #[error("missing template variable: {0}")]
    MissingVar(String),
    #[error("render error: {0}")]
    Render(String),
    #[error("duplicate content entry: {0}")]
    Duplicate(String),
}

static REGISTRY: OnceLock<ContentRegistry> = OnceLock::new();

pub fn registry() -> &'static ContentRegistry {
    REGISTRY.get_or_init(|| {
        ContentRegistry::load().expect("Failed to load gateway content registry")
    })
}

pub fn message_text(
    id: &str,
    interface: Option<&str>,
    vars: &[(&str, &str)],
) -> Result<String, ContentError> {
    registry().message_text(id, interface, vars)
}

pub fn prompt_text(
    id: &str,
    provider: Option<&str>,
    vars: &[(&str, &str)],
) -> Result<String, ContentError> {
    registry().prompt_text(id, provider, vars)
}

#[cfg(test)]
mod tests {
    use super::{ids, registry};

    #[test]
    fn test_registry_loads_message_and_prompt() {
        let reg = registry();
        let msg = reg.message_text(ids::ERROR_GENERIC, None, &[]);
        assert!(msg.is_ok());

        let prompt = reg.prompt_text(ids::PROMPT_SYSTEM_BASE, None, &[]);
        assert!(prompt.is_ok());
    }
}
