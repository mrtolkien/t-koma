//! Skill loading tool.
//!
//! This tool allows the agent to load skill content from the skill registry.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::Tool;

/// Tool for loading skill content.
///
/// This tool allows the agent to request the full content of a skill
/// when it needs to use it.
#[derive(Debug)]
pub struct LoadSkillTool {
    /// Path to the skills directory
    skills_path: std::path::PathBuf,
}

impl LoadSkillTool {
    /// Create a new load skill tool.
    ///
    /// # Arguments
    ///
    /// * `skills_path` - Path to the directory containing skills
    pub fn new(skills_path: std::path::PathBuf) -> Self {
        Self { skills_path }
    }

    /// Get the skills path.
    pub fn skills_path(&self) -> &std::path::Path {
        &self.skills_path
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load the full content of a skill from the skill registry. \
         Use this when you need to use a skill that has been identified \
         but not yet loaded. Returns the complete SKILL.md content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill to load (e.g., 'skill-creator')"
                }
            },
            "required": ["skill_name"]
        })
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(
            "When you identify that a skill is needed for a task, use the load_skill tool \
             to get the full content of the skill. The skill_name parameter should be \
             the exact name from the available skills list. After loading, follow the \
             instructions in the skill content to complete the task.",
        )
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
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

        // Look for the skill in the skills directory
        let skill_path = self.skills_path.join(skill_name).join("SKILL.md");

        if !skill_path.exists() {
            return Err(format!("Skill '{}' not found at {:?}", skill_name, skill_path));
        }

        // Read the skill content
        match tokio::fs::read_to_string(&skill_path).await {
            Ok(content) => Ok(content),
            Err(e) => Err(format!("Failed to read skill '{}': {}", skill_name, e)),
        }
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
        let skill_dir = temp_dir.path().join("test-skill");
        std::fs::create_dir(&skill_dir).unwrap();

        let skill_content = r#"---
name: test-skill
description: A test skill.
---

# Test Skill

This is the skill content."#;

        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let tool = LoadSkillTool::new(temp_dir.path().to_path_buf());
        let args = json!({"skill_name": "test-skill"});

        let result = tool.execute(args).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Test Skill"));
    }

    #[tokio::test]
    async fn test_load_skill_not_found() {
        let temp_dir = TempDir::new().unwrap();

        let tool = LoadSkillTool::new(temp_dir.path().to_path_buf());
        let args = json!({"skill_name": "nonexistent"});

        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_load_skill_missing_param() {
        let temp_dir = TempDir::new().unwrap();

        let tool = LoadSkillTool::new(temp_dir.path().to_path_buf());
        let args = json!({});

        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing"));
    }

    #[test]
    fn test_tool_definition() {
        let tool = LoadSkillTool::new(std::path::PathBuf::from("/tmp"));

        assert_eq!(tool.name(), "load_skill");
        assert!(!tool.description().is_empty());
        assert!(tool.prompt().is_some());

        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }
}
