use std::collections::BTreeMap;

use serde::Deserialize;

use super::{ContentError, ContentScope};
use crate::content::template::{render_template, TemplateVars};

#[derive(Debug, Clone)]
pub struct PromptTemplate {
    pub id: String,
    pub scope: ContentScope,
    pub role: Option<String>,
    pub vars: Vec<String>,
    pub inputs: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptFrontMatter {
    pub id: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub role: Option<String>,
    #[serde(default)]
    pub vars: Option<Vec<String>>,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

impl PromptTemplate {
    pub fn from_parts(front_matter: PromptFrontMatter, body: String) -> Result<Self, ContentError> {
        let scope = ContentScope::parse(front_matter.scope.as_deref().unwrap_or("shared"))?;
        Ok(Self {
            id: front_matter.id,
            scope,
            role: front_matter.role,
            vars: front_matter.vars.unwrap_or_default(),
            inputs: front_matter.inputs,
            body,
        })
    }

    pub fn render(&self, vars: &TemplateVars) -> Result<String, ContentError> {
        render_template(&self.body, vars)
    }
}
