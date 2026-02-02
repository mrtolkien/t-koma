//! Pending users management with auto-pruning.
//!
//! Pending users are stored separately from approved users and are
//! automatically pruned after 1 hour.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long pending users remain valid
const PENDING_EXPIRY_HOURS: i64 = 1;

/// Pending user entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUser {
    pub id: String,
    pub name: String,
    pub requested_at: DateTime<Utc>,
}

impl PendingUser {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            requested_at: Utc::now(),
        }
    }

    /// Check if this pending user has expired (> 1 hour old)
    pub fn is_expired(&self) -> bool {
        Utc::now() - self.requested_at > Duration::hours(PENDING_EXPIRY_HOURS)
    }
}

/// Pending users storage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingUsers {
    #[serde(default)]
    pub users: HashMap<String, PendingUser>,
}

impl PendingUsers {
    /// Load pending users from disk, pruning expired entries
    pub fn load() -> Result<Self, PendingError> {
        let path = Self::pending_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)?;
        let mut pending: Self = toml::from_str(&content)?;

        // Prune expired users
        let before_count = pending.users.len();
        pending.users.retain(|_, u| !u.is_expired());
        let after_count = pending.users.len();

        if before_count != after_count {
            tracing::info!(
                "Pruned {} expired pending users",
                before_count - after_count
            );
            // Save pruned list
            pending.save()?;
        }

        Ok(pending)
    }

    /// Save pending users to disk
    pub fn save(&self) -> Result<(), PendingError> {
        let path = Self::pending_path()?;

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

    /// Get pending file path
    fn pending_path() -> Result<PathBuf, PendingError> {
        let dirs = dirs::config_dir().ok_or(PendingError::NoConfigDir)?;
        Ok(dirs.join("t-koma").join("pending.toml"))
    }

    /// Add a pending user (if not already present)
    pub fn add(&mut self, id: impl Into<String>, name: impl Into<String>) -> &PendingUser {
        let id = id.into();
        let user = PendingUser::new(&id, name);
        self.users.entry(id).or_insert(user)
    }

    /// Get a pending user
    pub fn get(&self, id: &str) -> Option<&PendingUser> {
        self.users.get(id)
    }

    /// Remove a pending user (returns the user if found)
    pub fn remove(&mut self, id: &str) -> Option<PendingUser> {
        self.users.remove(id)
    }

    /// Get all pending users as a sorted list (oldest first)
    pub fn list(&self) -> Vec<&PendingUser> {
        let mut list: Vec<_> = self.users.values().collect();
        list.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        list
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    /// Count
    pub fn len(&self) -> usize {
        self.users.len()
    }
}

/// Pending users errors
#[derive(Debug, thiserror::Error)]
pub enum PendingError {
    #[error("No config directory found")]
    NoConfigDir,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_user_creation() {
        let user = PendingUser::new("123", "test");
        assert_eq!(user.id, "123");
        assert_eq!(user.name, "test");
        assert!(!user.is_expired());
    }

    #[test]
    fn test_expiry() {
        let mut user = PendingUser::new("123", "test");
        // Set requested_at to 2 hours ago
        user.requested_at = Utc::now() - Duration::hours(2);
        assert!(user.is_expired());
    }
}
