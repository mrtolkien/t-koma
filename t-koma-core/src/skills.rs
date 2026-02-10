//! Agent Skills system implementation.
//!
//! This module provides support for the Agent Skills specification,
//! allowing the agent to discover, load, and use specialized skills
//! stored in the configuration folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use tracing::warn;

/// Errors that can occur when working with skills.
#[derive(Error, Debug)]
pub enum SkillError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parsing error: {0}")]
    YamlParse(String),
    #[error("Invalid skill format: {0}")]
    InvalidFormat(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Skill not found: {0}")]
    NotFound(String),
}

/// Represents a loaded skill with its metadata.
///
/// Skills are self-contained directories with a `SKILL.md` file
/// that includes YAML frontmatter followed by Markdown content.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Skill name (from frontmatter)
    pub name: String,
    /// Skill description (from frontmatter)
    pub description: String,
    /// Optional license information
    pub license: Option<String>,
    /// Optional compatibility notes
    pub compatibility: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Path to the skill directory
    pub path: PathBuf,
    /// Full content of SKILL.md (loaded on demand)
    pub content: Option<String>,
}

impl Skill {
    /// Creates an empty skill with the given path.
    fn new(path: PathBuf) -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            license: None,
            compatibility: None,
            metadata: HashMap::new(),
            path,
            content: None,
        }
    }

    /// Parse a skill from a SKILL.md file path.
    ///
    /// This reads the file and extracts the YAML frontmatter to populate
    /// the skill metadata. The full content is not loaded until
    /// `load_content()` is called.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SKILL.md file
    ///
    /// # Returns
    ///
    /// Returns `Ok(Skill)` if parsing succeeds, or `Err(SkillError)` if
    /// the file cannot be read or the frontmatter is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use t_koma_core::skills::Skill;
    /// use std::path::Path;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let skill = Skill::from_file(Path::new("prompts/skills/my-skill/SKILL.md"))?;
    /// println!("Loaded skill: {}", skill.name);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file(path: &Path) -> Result<Self, SkillError> {
        let content = fs::read_to_string(path)?;
        let mut skill = Self::new(path.parent().unwrap_or(Path::new("")).to_path_buf());
        skill.parse_frontmatter(&content)?;
        Ok(skill)
    }

    /// Parse the YAML frontmatter from the content.
    fn parse_frontmatter(&mut self, content: &str) -> Result<(), SkillError> {
        // Check for frontmatter delimiters
        if !content.starts_with("---") {
            return Err(SkillError::InvalidFormat(
                "SKILL.md must start with YAML frontmatter (---)".to_string(),
            ));
        }

        // Find the end of frontmatter
        let end_idx = content[3..].find("---").ok_or_else(|| {
            SkillError::InvalidFormat("Frontmatter not properly closed".to_string())
        })?;

        let frontmatter = &content[3..end_idx + 3];

        // Parse YAML
        let docs = yaml_rust2::YamlLoader::load_from_str(frontmatter)
            .map_err(|e| SkillError::YamlParse(e.to_string()))?;

        let doc = docs
            .first()
            .ok_or_else(|| SkillError::InvalidFormat("Empty frontmatter".to_string()))?;

        // Extract required fields
        self.name = doc["name"]
            .as_str()
            .ok_or_else(|| SkillError::MissingField("name".to_string()))?
            .to_string();

        // Validate name format
        Self::validate_name(&self.name)?;

        self.description = doc["description"]
            .as_str()
            .ok_or_else(|| SkillError::MissingField("description".to_string()))?
            .to_string();

        // Extract optional fields
        self.license = doc["license"].as_str().map(|s| s.to_string());
        self.compatibility = doc["compatibility"].as_str().map(|s| s.to_string());

        // Extract metadata
        if let Some(metadata) = doc["metadata"].as_hash() {
            for (key, value) in metadata {
                if let (Some(k), Some(v)) = (key.as_str(), value.as_str()) {
                    self.metadata.insert(k.to_string(), v.to_string());
                }
            }
        }

        Ok(())
    }

    /// Validate the skill name according to the specification.
    ///
    /// Rules:
    /// - Must be 1-64 characters
    /// - May only contain lowercase alphanumeric characters and hyphens
    /// - Must not start or end with hyphen
    /// - Must not contain consecutive hyphens
    fn validate_name(name: &str) -> Result<(), SkillError> {
        if name.is_empty() || name.len() > 64 {
            return Err(SkillError::InvalidFormat(
                "Skill name must be 1-64 characters".to_string(),
            ));
        }

        if name.starts_with('-') || name.ends_with('-') {
            return Err(SkillError::InvalidFormat(
                "Skill name must not start or end with hyphen".to_string(),
            ));
        }

        if name.contains("--") {
            return Err(SkillError::InvalidFormat(
                "Skill name must not contain consecutive hyphens".to_string(),
            ));
        }

        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(SkillError::InvalidFormat(
                "Skill name may only contain lowercase alphanumeric characters and hyphens"
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Load the full content of the skill from disk.
    ///
    /// This loads the entire SKILL.md file content, which can be used
    /// by the agent when the skill is activated.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the content is loaded successfully.
    pub fn load_content(&mut self) -> Result<(), SkillError> {
        let skill_md_path = self.path.join("SKILL.md");
        let content = fs::read_to_string(&skill_md_path)?;
        self.content = Some(content);
        Ok(())
    }

    /// Get the full content of the skill, loading it if necessary.
    ///
    /// # Returns
    ///
    /// Returns `Some(&str)` with the content if available, or `None` if
    /// loading fails.
    pub fn get_content(&mut self) -> Option<&str> {
        if self.content.is_none()
            && let Err(e) = self.load_content()
        {
            warn!("Failed to load skill content for '{}': {}", self.name, e);
            return None;
        }
        self.content.as_deref()
    }

    /// List available scripts in the skill's `scripts/` directory.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to script files.
    pub fn list_scripts(&self) -> Vec<PathBuf> {
        self.list_directory("scripts")
    }

    /// List available references in the skill's `references/` directory.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to reference files.
    pub fn list_references(&self) -> Vec<PathBuf> {
        self.list_directory("references")
    }

    /// List available assets in the skill's `assets/` directory.
    ///
    /// # Returns
    ///
    /// Returns a vector of paths to asset files.
    pub fn list_assets(&self) -> Vec<PathBuf> {
        self.list_directory("assets")
    }

    /// Helper to list files in a subdirectory.
    fn list_directory(&self, dir_name: &str) -> Vec<PathBuf> {
        let dir_path = self.path.join(dir_name);
        if !dir_path.exists() || !dir_path.is_dir() {
            return Vec::new();
        }

        fs::read_dir(&dir_path)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Read a reference file from the skill's `references/` directory.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the reference file (e.g., "REFERENCE.md")
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` with the file content, or `Err(SkillError)`
    /// if the file doesn't exist or cannot be read.
    pub fn read_reference(&self, name: &str) -> Result<String, SkillError> {
        let path = safe_child_file_path(&self.path.join("references"), name)?;
        fs::read_to_string(&path).map_err(SkillError::Io)
    }

    /// Read a script file from the skill's `scripts/` directory.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the script file (e.g., "extract.py")
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` with the file content, or `Err(SkillError)`
    /// if the file doesn't exist or cannot be read.
    pub fn read_script(&self, name: &str) -> Result<String, SkillError> {
        let path = safe_child_file_path(&self.path.join("scripts"), name)?;
        fs::read_to_string(&path).map_err(SkillError::Io)
    }
}

fn safe_child_file_path(base_dir: &Path, name: &str) -> Result<PathBuf, SkillError> {
    let path = Path::new(name);
    let mut components = path.components();
    let valid_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();

    if !valid_name {
        return Err(SkillError::InvalidFormat(format!(
            "Invalid file name '{}': nested, absolute, and parent paths are not allowed",
            name
        )));
    }

    Ok(base_dir.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_skill_file(dir: &TempDir, content: &str) -> PathBuf {
        let skill_dir = dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();
        let skill_md = skill_dir.join("SKILL.md");
        let mut file = fs::File::create(&skill_md).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        skill_md
    }

    fn create_basic_skill(dir: &TempDir) -> Skill {
        let path = create_test_skill_file(
            dir,
            r#"---
name: test-skill
description: A test skill.
---
"#,
        );
        Skill::from_file(&path).unwrap()
    }

    #[test]
    fn test_parse_valid_skill() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
name: test-skill
description: A test skill for unit testing.
license: MIT
metadata:
  author: test
  version: "1.0"
---

# Test Skill

This is a test skill.
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let skill = Skill::from_file(&path).unwrap();

        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill for unit testing.");
        assert_eq!(skill.license, Some("MIT".to_string()));
        assert_eq!(skill.metadata.get("author"), Some(&"test".to_string()));
        assert_eq!(skill.metadata.get("version"), Some(&"1.0".to_string()));
    }

    #[test]
    fn test_parse_missing_name() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
description: A test skill without a name.
---

# Test Skill
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let result = Skill::from_file(&path);
        assert!(matches!(result, Err(SkillError::MissingField(_))));
    }

    #[test]
    fn test_parse_missing_description() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
name: test-skill
---

# Test Skill
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let result = Skill::from_file(&path);
        assert!(matches!(result, Err(SkillError::MissingField(_))));
    }

    #[test]
    fn test_parse_invalid_name_uppercase() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
name: Test-Skill
description: A test skill with uppercase.
---

# Test Skill
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let result = Skill::from_file(&path);
        assert!(matches!(result, Err(SkillError::InvalidFormat(_))));
    }

    #[test]
    fn test_parse_invalid_name_starting_hyphen() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
name: -test-skill
description: A test skill starting with hyphen.
---

# Test Skill
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let result = Skill::from_file(&path);
        assert!(matches!(result, Err(SkillError::InvalidFormat(_))));
    }

    #[test]
    fn test_parse_invalid_name_consecutive_hyphens() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"---
name: test--skill
description: A test skill with consecutive hyphens.
---

# Test Skill
"#;
        let path = create_test_skill_file(&temp_dir, content);

        let result = Skill::from_file(&path);
        assert!(matches!(result, Err(SkillError::InvalidFormat(_))));
    }

    #[test]
    fn test_list_scripts() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();

        // Create SKILL.md
        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            r#"---
name: test-skill
description: A test skill.
---
"#,
        )
        .unwrap();

        // Create scripts directory with files
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("script1.py"), "# script 1").unwrap();
        fs::write(scripts_dir.join("script2.sh"), "# script 2").unwrap();

        let skill = Skill::from_file(&skill_md).unwrap();
        let scripts = skill.list_scripts();

        assert_eq!(scripts.len(), 2);
    }

    #[test]
    fn test_list_references() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();

        // Create SKILL.md
        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            r#"---
name: test-skill
description: A test skill.
---
"#,
        )
        .unwrap();

        // Create references directory with files
        let refs_dir = skill_dir.join("references");
        fs::create_dir(&refs_dir).unwrap();
        fs::write(refs_dir.join("REFERENCE.md"), "# Reference").unwrap();

        let skill = Skill::from_file(&skill_md).unwrap();
        let refs = skill.list_references();

        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_read_reference_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let skill = create_basic_skill(&temp_dir);
        for path in ["../secret.txt", "nested/file.md", absolute_test_path()] {
            let err = skill.read_reference(path).unwrap_err();
            assert!(matches!(err, SkillError::InvalidFormat(_)));
        }
    }

    #[test]
    fn test_read_script_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let skill = create_basic_skill(&temp_dir);
        for path in ["../secret.sh", "nested/run.sh", absolute_test_path()] {
            let err = skill.read_script(path).unwrap_err();
            assert!(matches!(err, SkillError::InvalidFormat(_)));
        }
    }

    #[cfg(target_family = "windows")]
    fn absolute_test_path() -> &'static str {
        "C:\\secret.txt"
    }

    #[cfg(not(target_family = "windows"))]
    fn absolute_test_path() -> &'static str {
        "/secret.txt"
    }
}
