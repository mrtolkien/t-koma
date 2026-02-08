//! Skill loading tool.
//!
//! This tool allows the agent to load skill content from one of several
//! skill directories, searched in priority order. Ghost-local skills
//! (from the workspace) override user config and project defaults.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Tool, ToolContext};

/// Tool for loading skill content.
///
/// Searches multiple skill directories in priority order:
/// 1. Ghost workspace `skills/` (highest priority, from ToolContext)
/// 2. User config skills
/// 3. Project default skills
#[derive(Debug)]
pub struct LoadSkillTool {
    /// Skill directories searched in priority order (first match wins)
    paths: Vec<std::path::PathBuf>,
}

impl LoadSkillTool {
    /// Create a new load skill tool.
    ///
    /// # Arguments
    ///
    /// * `paths` - Directories to search for skills, in priority order
    pub fn new(paths: Vec<std::path::PathBuf>) -> Self {
        Self { paths }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load the full content of a skill for detailed guidance on a workflow. \
         Use this when you need to use a skill that has been identified \
         but not yet loaded. Returns the complete SKILL.md content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill to load (e.g., 'note-writer')"
                }
            },
            "required": ["skill_name"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let skill_name = args["skill_name"]
            .as_str()
            .ok_or_else(|| "Missing 'skill_name' parameter".to_string())?;

        // Validate skill name format
        if skill_name.is_empty() || skill_name.len() > 64 {
            return Err("Skill name must be 1-64 characters".to_string());
        }

        if !skill_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(
                "Skill name may only contain lowercase alphanumeric characters and hyphens"
                    .to_string(),
            );
        }

        // Build search paths: workspace skills first (highest priority), then configured paths
        let workspace_skills = context.workspace_root().join("skills");
        let mut search_paths = vec![workspace_skills];
        search_paths.extend(self.paths.iter().cloned());

        for dir in &search_paths {
            let skill_path = dir.join(skill_name).join("SKILL.md");
            if skill_path.exists() {
                return tokio::fs::read_to_string(&skill_path)
                    .await
                    .map_err(|e| format!("Failed to read skill '{}': {}", skill_name, e));
            }
        }

        Err(format!(
            "Skill '{}' not found. Searched {} directories.",
            skill_name,
            search_paths.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_load_skill_success() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir(&skill_dir).unwrap();

        let skill_content = r#"---
name: test-skill
description: A test skill.
---

# Test Skill

This is the skill content."#;

        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let tool = LoadSkillTool::new(vec![temp_dir.path().to_path_buf()]);
        let args = json!({"skill_name": "test-skill"});

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Test Skill"));
    }

    #[tokio::test]
    async fn test_load_skill_workspace_priority() {
        let temp_dir = TempDir::new().unwrap();

        // Create a workspace skill (in the temp dir root which doubles as workspace)
        let ws_skills = temp_dir.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&ws_skills).unwrap();
        std::fs::write(ws_skills.join("SKILL.md"), "workspace version").unwrap();

        // Create a config skill with same name
        let config_dir = temp_dir.path().join("config-skills").join("my-skill");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SKILL.md"), "config version").unwrap();

        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let tool = LoadSkillTool::new(vec![temp_dir.path().join("config-skills")]);
        let args = json!({"skill_name": "my-skill"});

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("workspace version"));
    }

    #[tokio::test]
    async fn test_load_skill_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = LoadSkillTool::new(vec![temp_dir.path().to_path_buf()]);
        let args = json!({"skill_name": "nonexistent"});

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_load_skill_missing_param() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let tool = LoadSkillTool::new(vec![]);
        let args = json!({});

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing"));
    }

    #[test]
    fn test_tool_definition() {
        let tool = LoadSkillTool::new(vec![]);

        assert_eq!(tool.name(), "load_skill");
        assert!(!tool.description().is_empty());

        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }
}
