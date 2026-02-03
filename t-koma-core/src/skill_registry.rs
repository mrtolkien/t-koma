//! Skill Registry for managing available skills.
//!
//! The `SkillRegistry` provides skill discovery and management,
//! loading skill metadata at startup and full content on demand.

use crate::skills::{Skill, SkillError};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Registry for managing available skills.
///
/// The registry discovers skills from multiple directories and provides
/// methods to access them by name or list all available skills.
/// Skills from the config directory take precedence over project skills.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    /// Map of skill name to skill metadata
    skills: HashMap<String, Skill>,
    /// Project-level skills path (./default-prompts/skills/)
    project_path: Option<PathBuf>,
    /// User config skills path (~/.config/t-koma/skills/)
    config_path: Option<PathBuf>,
}

impl SkillRegistry {
    /// Creates a new registry with default paths.
    ///
    /// Discovers skills from both the project directory (`./default-prompts/skills/`)
    /// and the user config directory (`~/.config/t-koma/skills/`).
    /// Config skills take precedence over project skills.
    ///
    /// # Returns
    ///
    /// Returns `Ok(SkillRegistry)` with discovered skills.
    ///
    /// # Example
    ///
    /// ```
    /// use t_koma_core::skill_registry::SkillRegistry;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let registry = SkillRegistry::new()?;
    /// println!("Discovered {} skills", registry.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Result<Self, SkillError> {
        let project_path = PathBuf::from("./default-prompts/skills");
        let config_path = dirs::config_dir().map(|d| d.join("t-koma").join("skills"));

        Self::new_with_paths(
            project_path.exists().then_some(project_path),
            config_path.filter(|p| p.exists()),
        )
    }

    /// Creates a new registry with explicit paths.
    ///
    /// # Arguments
    ///
    /// * `project_path` - Optional path to project-level skills directory
    /// * `config_path` - Optional path to user config skills directory
    ///
    /// # Returns
    ///
    /// Returns `Ok(SkillRegistry)` with discovered skills from all provided paths.
    /// Config skills take precedence over project skills.
    pub fn new_with_paths(
        project_path: Option<PathBuf>,
        config_path: Option<PathBuf>,
    ) -> Result<Self, SkillError> {
        let mut registry = Self {
            skills: HashMap::new(),
            project_path,
            config_path,
        };
        registry.discover_skills()?;
        Ok(registry)
    }

    /// Creates an empty registry without discovering skills.
    ///
    /// This is useful for testing or when skills will be added manually.
    pub fn empty() -> Self {
        Self {
            skills: HashMap::new(),
            project_path: None,
            config_path: None,
        }
    }

    /// Discover all skills from configured paths.
    ///
    /// Skills are discovered from both project and config paths.
    /// Config skills take precedence and will override project skills
    /// with the same name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if discovery completes.
    pub fn discover_skills(&mut self) -> Result<(), SkillError> {
        // Clone paths to avoid borrow issues
        let project_path = self.project_path.clone();
        let config_path = self.config_path.clone();

        // First, discover project-level skills
        if let Some(ref path) = project_path
            && path.exists()
        {
            info!("Discovering project skills from: {:?}", path);
            self.discover_from_path(path)?;
        }

        // Then, discover config skills (these take precedence)
        if let Some(ref path) = config_path
            && path.exists()
        {
            info!("Discovering config skills from: {:?}", path);
            self.discover_from_path(path)?;
        }

        info!("Total skills discovered: {}", self.skills.len());
        Ok(())
    }

    /// Discover skills from a specific path.
    fn discover_from_path(&mut self, skills_path: &Path) -> Result<(), SkillError> {
        let entries = fs::read_dir(skills_path)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            match Skill::from_file(&skill_md) {
                Ok(skill) => {
                    // Insert or override existing skill
                    self.skills.insert(skill.name.clone(), skill);
                }
                Err(e) => {
                    warn!("Failed to parse skill at {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }

    /// Get a skill by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The skill name
    ///
    /// # Returns
    ///
    /// Returns `Some(&Skill)` if found, or `None` if not found.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get a mutable reference to a skill by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The skill name
    ///
    /// # Returns
    ///
    /// Returns `Some(&mut Skill)` if found, or `None` if not found.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Skill> {
        self.skills.get_mut(name)
    }

    /// Load full content of a specific skill.
    ///
    /// This loads the entire SKILL.md file content for the skill,
    /// which can then be accessed via `skill.content`.
    ///
    /// # Arguments
    ///
    /// * `name` - The skill name
    ///
    /// # Returns
    ///
    /// Returns `Ok(&Skill)` with loaded content, or `Err(SkillError)`
    /// if the skill is not found or content cannot be loaded.
    pub fn load_skill(&mut self, name: &str) -> Result<&Skill, SkillError> {
        let skill = self
            .skills
            .get_mut(name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))?;

        skill.load_content()?;
        Ok(skill)
    }

    /// Get all skill names and descriptions.
    ///
    /// This is useful for including in the system prompt
    /// to make the agent aware of available skills.
    ///
    /// # Returns
    ///
    /// Returns a vector of `(name, description)` tuples.
    pub fn list_skills(&self) -> Vec<(String, String)> {
        self.skills
            .values()
            .map(|s| (s.name.clone(), s.description.clone()))
            .collect()
    }

    /// Get all skill names.
    ///
    /// # Returns
    ///
    /// Returns a vector of skill names.
    pub fn list_skill_names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    /// Check if a skill exists.
    ///
    /// # Arguments
    ///
    /// * `name` - The skill name
    ///
    /// # Returns
    ///
    /// Returns `true` if the skill exists, `false` otherwise.
    pub fn has_skill(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Get the number of skills in the registry.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Get the project skills path.
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }

    /// Get the config skills path.
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Get an iterator over all skills.
    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    /// Search skills by keyword in name or description.
    ///
    /// # Arguments
    ///
    /// * `keyword` - The keyword to search for (case-insensitive)
    ///
    /// # Returns
    ///
    /// Returns a vector of skills matching the keyword.
    pub fn search(&self, keyword: &str) -> Vec<&Skill> {
        let keyword_lower = keyword.to_lowercase();
        self.skills
            .values()
            .filter(|s| {
                s.name.to_lowercase().contains(&keyword_lower)
                    || s.description.to_lowercase().contains(&keyword_lower)
            })
            .collect()
    }

    /// Ensure the config skills directory exists.
    ///
    /// Creates the directory structure if it doesn't exist.
    ///
    /// # Returns
    ///
    /// Returns the config path if successful.
    pub fn ensure_config_dir(&self) -> Result<Option<PathBuf>, SkillError> {
        if let Some(ref path) = self.config_path {
            if !path.exists() {
                fs::create_dir_all(path)?;
                info!("Created config skills directory: {:?}", path);
            }
            return Ok(Some(path.clone()));
        }
        Ok(None)
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_skill_dir(base: &Path, name: &str, content: &str) -> PathBuf {
        let skill_dir = base.join(name);
        fs::create_dir(&skill_dir).unwrap();
        let skill_md = skill_dir.join("SKILL.md");
        let mut file = fs::File::create(&skill_md).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        skill_dir
    }

    #[test]
    fn test_discover_from_single_path() {
        let temp_dir = TempDir::new().unwrap();

        create_skill_dir(
            temp_dir.path(),
            "skill-one",
            r#"---
name: skill-one
description: The first test skill.
---

# Skill One
"#,
        );

        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.has_skill("skill-one"));
    }

    #[test]
    fn test_config_takes_precedence() {
        let project_dir = TempDir::new().unwrap();
        let config_dir = TempDir::new().unwrap();

        // Create skill in project
        create_skill_dir(
            project_dir.path(),
            "my-skill",
            r#"---
name: my-skill
description: Project version.
---
"#,
        );

        // Create same skill in config with different description
        create_skill_dir(
            config_dir.path(),
            "my-skill",
            r#"---
name: my-skill
description: Config version.
---
"#,
        );

        let registry = SkillRegistry::new_with_paths(
            Some(project_dir.path().to_path_buf()),
            Some(config_dir.path().to_path_buf()),
        )
        .unwrap();

        // Config version should take precedence
        let skill = registry.get("my-skill").unwrap();
        assert_eq!(skill.description, "Config version.");
    }

    #[test]
    fn test_discover_skills() {
        let temp_dir = TempDir::new().unwrap();

        // Create two valid skills
        create_skill_dir(
            temp_dir.path(),
            "skill-one",
            r#"---
name: skill-one
description: The first test skill.
---

# Skill One
"#,
        );

        create_skill_dir(
            temp_dir.path(),
            "skill-two",
            r#"---
name: skill-two
description: The second test skill.
---

# Skill Two
"#,
        );

        // Create a directory without SKILL.md (should be skipped)
        fs::create_dir(temp_dir.path().join("not-a-skill")).unwrap();

        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.has_skill("skill-one"));
        assert!(registry.has_skill("skill-two"));
        assert!(!registry.has_skill("not-a-skill"));
    }

    #[test]
    fn test_get_skill() {
        let temp_dir = TempDir::new().unwrap();

        create_skill_dir(
            temp_dir.path(),
            "test-skill",
            r#"---
name: test-skill
description: A test skill.
---

# Test Skill
"#,
        );

        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        let skill = registry.get("test-skill");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "test-skill");

        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_list_skills() {
        let temp_dir = TempDir::new().unwrap();

        create_skill_dir(
            temp_dir.path(),
            "skill-a",
            r#"---
name: skill-a
description: Skill A description.
---
"#,
        );

        create_skill_dir(
            temp_dir.path(),
            "skill-b",
            r#"---
name: skill-b
description: Skill B description.
---
"#,
        );

        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        let skills = registry.list_skills();
        assert_eq!(skills.len(), 2);

        let names: Vec<String> = skills.iter().map(|(n, _)| n.clone()).collect();
        assert!(names.contains(&"skill-a".to_string()));
        assert!(names.contains(&"skill-b".to_string()));
    }

    #[test]
    fn test_search_skills() {
        let temp_dir = TempDir::new().unwrap();

        create_skill_dir(
            temp_dir.path(),
            "pdf-processor",
            r#"---
name: pdf-processor
description: Extract text and tables from PDF files.
---
"#,
        );

        create_skill_dir(
            temp_dir.path(),
            "image-resizer",
            r#"---
name: image-resizer
description: Resize and convert image files.
---
"#,
        );

        create_skill_dir(
            temp_dir.path(),
            "data-analysis",
            r#"---
name: data-analysis
description: Analyze CSV and Excel data files.
---
"#,
        );

        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        let pdf_results = registry.search("pdf");
        assert_eq!(pdf_results.len(), 1);
        assert_eq!(pdf_results[0].name, "pdf-processor");

        let file_results = registry.search("files");
        assert_eq!(file_results.len(), 3); // All three skills mention "files"

        let image_results = registry.search("image");
        assert_eq!(image_results.len(), 1);
    }

    #[test]
    fn test_load_skill_content() {
        let temp_dir = TempDir::new().unwrap();

        create_skill_dir(
            temp_dir.path(),
            "content-skill",
            r#"---
name: content-skill
description: A skill with content.
---

# Content Section

This is the skill content.
"#,
        );

        let mut registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        // Initially content is not loaded
        let skill = registry.get("content-skill").unwrap();
        assert!(skill.content.is_none());

        // Load the content
        let skill = registry.load_skill("content-skill").unwrap();
        assert!(skill.content.is_some());
        assert!(
            skill
                .content
                .as_ref()
                .unwrap()
                .contains("# Content Section")
        );
    }

    #[test]
    fn test_empty_registry() {
        let temp_dir = TempDir::new().unwrap();
        let registry =
            SkillRegistry::new_with_paths(Some(temp_dir.path().to_path_buf()), None).unwrap();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_nonexistent_paths() {
        let registry = SkillRegistry::new_with_paths(
            Some(PathBuf::from("/nonexistent/path")),
            Some(PathBuf::from("/another/nonexistent/path")),
        )
        .unwrap();

        assert!(registry.is_empty());
    }

    #[test]
    fn test_ensure_config_dir() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test-config").join("skills");

        let registry = SkillRegistry::new_with_paths(None, Some(config_path.clone())).unwrap();

        assert!(!config_path.exists());
        let result = registry.ensure_config_dir().unwrap();
        assert!(result.is_some());
        assert!(config_path.exists());
    }
}
