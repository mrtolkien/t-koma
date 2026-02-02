//! Persistent configuration with approved user management.
//!
//! Stores config in XDG directories:
//! - Linux: ~/.config/t-koma/config.toml
//! - macOS: ~/Library/Application Support/t-koma/config.toml
//! - Windows: %APPDATA%\t-koma\config.toml

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Approved user entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedUser {
    pub id: String,
    pub name: String,
    pub approved_at: DateTime<Utc>,
    /// Whether user has been welcomed (sent first message after approval)
    #[serde(default)]
    pub welcomed: bool,
}

impl ApprovedUser {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            approved_at: Utc::now(),
            welcomed: false,
        }
    }
}

/// Approved users storage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovedUsers {
    #[serde(default)]
    pub users: HashMap<String, ApprovedUser>,
}

impl ApprovedUsers {
    /// Check if a user is approved
    pub fn is_approved(&self, user_id: &str) -> bool {
        self.users.contains_key(user_id)
    }

    /// Get a mutable reference to mark welcomed
    pub fn get_mut(&mut self, user_id: &str) -> Option<&mut ApprovedUser> {
        self.users.get_mut(user_id)
    }

    /// Add or update an approved user
    pub fn add(&mut self, id: impl Into<String>, name: impl Into<String>) -> &ApprovedUser {
        let id = id.into();
        let user = ApprovedUser::new(&id, name);
        self.users.insert(id.clone(), user);
        self.users.get(&id).unwrap()
    }

    /// Remove an approved user
    pub fn remove(&mut self, user_id: &str) -> Option<ApprovedUser> {
        self.users.remove(user_id)
    }

    /// Get all approved users
    pub fn list(&self) -> Vec<&ApprovedUser> {
        self.users.values().collect()
    }
}

/// Persistent configuration (approved users only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentConfig {
    /// Auth secret for API (auto-generated)
    pub secret_key: String,

    /// Approved Discord users
    #[serde(default)]
    pub discord: ApprovedUsers,

    /// Approved API users
    #[serde(default)]
    pub api: ApprovedUsers,
}

impl PersistentConfig {
    /// Generate a new random secret key
    fn generate_secret() -> String {
        use rand::Rng;
        let bytes: [u8; 32] = rand::thread_rng().r#gen();
        hex::encode(bytes)
    }

    /// Load config from disk or create default
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;

        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let config: Self = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Save config to disk
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Get config file path
    fn config_path() -> Result<PathBuf, ConfigError> {
        let dirs = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
        Ok(dirs.join("t-koma").join("config.toml"))
    }
}

impl Default for PersistentConfig {
    fn default() -> Self {
        Self {
            secret_key: Self::generate_secret(),
            discord: ApprovedUsers::default(),
            api: ApprovedUsers::default(),
        }
    }
}

/// Config errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("No config directory found")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approved_user_creation() {
        let user = ApprovedUser::new("123", "test");
        assert_eq!(user.id, "123");
        assert_eq!(user.name, "test");
        assert!(!user.welcomed);
    }

    #[test]
    fn test_approved_users() {
        let mut users = ApprovedUsers::default();
        
        // Add user
        users.add("123", "test");
        assert!(users.is_approved("123"));
        
        // Mark welcomed
        if let Some(user) = users.get_mut("123") {
            user.welcomed = true;
        }
        assert!(users.users.get("123").unwrap().welcomed);
        
        // Remove user
        users.remove("123");
        assert!(!users.is_approved("123"));
    }
}
