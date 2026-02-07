//! Default skills embedded in the binary.
//!
//! This module provides default skills that are embedded at compile time
//! and can be written to the user's config directory on first run.

use crate::skills::SkillError;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

/// Represents a default skill embedded in the binary.
#[derive(Debug, Clone)]
pub struct DefaultSkill {
    /// Skill name
    pub name: &'static str,
    /// Full content of SKILL.md
    pub content: &'static str,
}

/// Default skills embedded at compile time.
pub const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        name: "skill-creator",
        content: include_str!("../../default-prompts/skills/skill-creator/SKILL.md"),
    },
    DefaultSkill {
        name: "reference-researcher",
        content: include_str!("../../default-prompts/skills/reference-researcher/SKILL.md"),
    },
];

/// Manager for default skills.
///
/// Handles writing embedded default skills to the config directory
/// if they don't already exist, allowing users to modify them while
/// preserving the original defaults.
#[derive(Debug)]
pub struct DefaultSkillsManager {
    /// Map of skill names to their embedded content
    skills: HashMap<&'static str, &'static str>,
}

impl DefaultSkillsManager {
    /// Creates a new manager with all default skills.
    pub fn new() -> Self {
        let mut skills = HashMap::new();
        for skill in DEFAULT_SKILLS {
            skills.insert(skill.name, skill.content);
        }
        Self { skills }
    }

    /// Get the list of default skill names.
    pub fn skill_names(&self) -> Vec<&'static str> {
        self.skills.keys().copied().collect()
    }

    /// Check if a skill is a default skill.
    pub fn is_default_skill(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Write all default skills to the config directory.
    ///
    /// Only writes skills that don't already exist.
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the config skills directory
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all skills are written successfully.
    pub fn write_all(&self, config_path: &Path) -> Result<(), SkillError> {
        // Ensure the config directory exists
        if !config_path.exists() {
            fs::create_dir_all(config_path)?;
            info!("Created config skills directory: {:?}", config_path);
        }

        for (name, content) in &self.skills {
            self.write_skill(config_path, name, content)?;
        }

        Ok(())
    }

    /// Write a specific default skill to the config directory.
    ///
    /// Only writes if the skill doesn't already exist.
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the config skills directory
    /// * `name` - Name of the skill
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if written, `Ok(false)` if already exists,
    /// or `Err(SkillError)` if writing fails.
    pub fn write_skill_if_missing(
        &self,
        config_path: &Path,
        name: &str,
    ) -> Result<bool, SkillError> {
        if let Some(content) = self.skills.get(name) {
            self.write_skill(config_path, name, content)
        } else {
            Err(SkillError::NotFound(name.to_string()))
        }
    }

    /// Internal method to write a skill file.
    fn write_skill(
        &self,
        config_path: &Path,
        name: &str,
        content: &str,
    ) -> Result<bool, SkillError> {
        let skill_dir = config_path.join(name);
        let skill_file = skill_dir.join("SKILL.md");

        // Skip if already exists
        if skill_file.exists() {
            debug!(
                "Skill '{}' already exists at {:?}, skipping",
                name, skill_file
            );
            return Ok(false);
        }

        // Create skill directory
        fs::create_dir_all(&skill_dir)?;

        // Write SKILL.md
        fs::write(&skill_file, content)?;

        info!("Wrote default skill '{}' to {:?}", name, skill_file);
        Ok(true)
    }

    /// Get the content of a default skill.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the skill
    ///
    /// # Returns
    ///
    /// Returns `Some(&str)` with the content if found, or `None`.
    pub fn get_content(&self, name: &str) -> Option<&'static str> {
        self.skills.get(name).copied()
    }
}

impl Default for DefaultSkillsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize default skills in the config directory.
///
/// This is a convenience function that writes all default skills
/// to the user's config directory if they don't already exist.
///
/// # Arguments
///
/// * `config_path` - Path to the config skills directory
///
/// # Returns
///
/// Returns `Ok(())` on success.
pub fn init_default_skills(config_path: &Path) -> Result<(), SkillError> {
    let manager = DefaultSkillsManager::new();
    manager.write_all(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_skills_list() {
        let manager = DefaultSkillsManager::new();
        let names = manager.skill_names();

        assert!(!names.is_empty());
        assert!(names.contains(&"skill-creator"));
    }

    #[test]
    fn test_is_default_skill() {
        let manager = DefaultSkillsManager::new();

        assert!(manager.is_default_skill("skill-creator"));
        assert!(!manager.is_default_skill("nonexistent"));
    }

    #[test]
    fn test_get_content() {
        let manager = DefaultSkillsManager::new();

        let content = manager.get_content("skill-creator");
        assert!(content.is_some());
        assert!(content.unwrap().contains("skill-creator"));

        assert!(manager.get_content("nonexistent").is_none());
    }

    #[test]
    fn test_write_all_skills() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DefaultSkillsManager::new();

        manager.write_all(temp_dir.path()).unwrap();

        // Check that skill directory was created
        let skill_dir = temp_dir.path().join("skill-creator");
        assert!(skill_dir.exists());

        // Check that SKILL.md was written
        let skill_file = skill_dir.join("SKILL.md");
        assert!(skill_file.exists());

        // Check content
        let content = fs::read_to_string(&skill_file).unwrap();
        assert!(content.contains("skill-creator"));
    }

    #[test]
    fn test_write_skips_existing() {
        let temp_dir = TempDir::new().unwrap();
        let manager = DefaultSkillsManager::new();

        // Create existing skill
        let skill_dir = temp_dir.path().join("skill-creator");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "existing content").unwrap();

        // Try to write
        let written = manager
            .write_skill_if_missing(temp_dir.path(), "skill-creator")
            .unwrap();

        assert!(!written);

        // Content should not be overwritten
        let content = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert_eq!(content, "existing content");
    }

    #[test]
    fn test_init_default_skills() {
        let temp_dir = TempDir::new().unwrap();

        init_default_skills(temp_dir.path()).unwrap();

        // Verify skills were written
        assert!(temp_dir.path().join("skill-creator").exists());
    }
}
