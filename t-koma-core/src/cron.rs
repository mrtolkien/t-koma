use std::path::Path;

use serde::Deserialize;

use crate::ModelAliases;

#[derive(Debug, Clone, Deserialize)]
pub struct CronPreToolCall {
    pub name: String,
    #[serde(default)]
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct CronFrontmatter {
    #[serde(default)]
    pub name: Option<String>,
    pub schedule: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub carry_last_output: bool,
    #[serde(default)]
    pub model_aliases: Option<ModelAliases>,
    #[serde(default)]
    pub pre_tools: Vec<CronPreToolCall>,
}

#[derive(Debug, Clone)]
pub struct ParsedCronJobFile {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    pub enabled: bool,
    pub carry_last_output: bool,
    pub model_aliases: Option<ModelAliases>,
    pub pre_tools: Vec<CronPreToolCall>,
}

#[derive(Debug, thiserror::Error)]
pub enum CronParseError {
    #[error("missing +++ frontmatter block")]
    MissingFrontmatter,
    #[error("invalid frontmatter TOML: {0}")]
    InvalidFrontmatterToml(String),
    #[error("missing required field: schedule")]
    MissingSchedule,
}

fn default_true() -> bool {
    true
}

pub fn parse_cron_job_markdown(
    path: &Path,
    content: &str,
) -> Result<ParsedCronJobFile, CronParseError> {
    let trimmed = content.trim_start();
    let Some(rest) = trimmed.strip_prefix("+++\n") else {
        return Err(CronParseError::MissingFrontmatter);
    };
    let Some(end_idx) = rest.find("\n+++\n") else {
        return Err(CronParseError::MissingFrontmatter);
    };

    let frontmatter_str = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    let fm: CronFrontmatter = toml::from_str(frontmatter_str)
        .map_err(|e| CronParseError::InvalidFrontmatterToml(e.to_string()))?;
    if fm.schedule.trim().is_empty() {
        return Err(CronParseError::MissingSchedule);
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("cron");
    Ok(ParsedCronJobFile {
        id: stem.to_string(),
        name: fm.name.unwrap_or_else(|| stem.to_string()),
        schedule: fm.schedule.trim().to_string(),
        prompt: body.trim().to_string(),
        enabled: fm.enabled,
        carry_last_output: fm.carry_last_output,
        model_aliases: fm.model_aliases,
        pre_tools: fm.pre_tools,
    })
}
