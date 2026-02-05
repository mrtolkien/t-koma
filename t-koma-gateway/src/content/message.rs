use serde::Deserialize;

use super::{ContentError, ContentScope};
use crate::content::template::{render_template, TemplateVars};

#[derive(Debug, Clone)]
pub struct MessageTemplate {
    pub id: String,
    pub scope: ContentScope,
    pub kind: Option<String>,
    pub vars: Vec<String>,
    pub content: MessageContent,
    pub presentation: Option<toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageEntryRaw {
    #[serde(default)]
    pub scope: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub vars: Option<Vec<String>>,
    #[serde(default, flatten)]
    pub content: MessageContent,
    #[serde(default)]
    pub presentation: Option<toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MessageContent {
    pub title: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub actions: Vec<MessageAction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageAction {
    pub id: String,
    pub label: String,
    pub intent: String,
}

impl MessageTemplate {
    pub fn from_entry(id: String, raw: MessageEntryRaw) -> Result<Self, ContentError> {
        let scope = ContentScope::parse(raw.scope.as_deref().unwrap_or("shared"))?;
        Ok(Self {
            id,
            scope,
            kind: raw.kind,
            vars: raw.vars.unwrap_or_default(),
            content: raw.content,
            presentation: raw.presentation,
        })
    }

    pub fn render_plain(&self, vars: &TemplateVars) -> Result<String, ContentError> {
        let title = match &self.content.title {
            Some(title) => Some(render_template(title, vars)?),
            None => None,
        };
        let body = match &self.content.body {
            Some(body) => Some(render_template(body, vars)?),
            None => None,
        };

        let mut text = String::new();
        if let Some(title) = title {
            text.push_str(&title);
        }
        if let Some(body) = body {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&body);
        }

        if text.is_empty() {
            return Err(ContentError::Render("Message content is empty".to_string()));
        }

        Ok(text)
    }
}
