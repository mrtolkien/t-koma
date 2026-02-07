use std::collections::BTreeMap;

use serde::Deserialize;

use super::{ContentError, ContentScope};
use crate::content::template::TemplateVars;

#[derive(Debug, Clone)]
pub struct PromptTemplate {
    pub id: String,
    pub scope: ContentScope,
    pub role: Option<String>,
    pub vars: Vec<String>,
    pub inputs: BTreeMap<String, String>,
    pub body: String,
    pub source_path: std::path::PathBuf,
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
    pub fn from_parts(
        front_matter: PromptFrontMatter,
        body: String,
        source_path: std::path::PathBuf,
    ) -> Result<Self, ContentError> {
        let scope = ContentScope::parse(front_matter.scope.as_deref().unwrap_or("shared"))?;
        Ok(Self {
            id: front_matter.id,
            scope,
            role: front_matter.role,
            vars: front_matter.vars.unwrap_or_default(),
            inputs: front_matter.inputs,
            body,
            source_path,
        })
    }

    pub fn render(&self, vars: &TemplateVars) -> Result<String, ContentError> {
        crate::content::template::render_template_with_includes(&self.body, vars, &self.source_path)
    }
}
