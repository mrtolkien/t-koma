//! User management operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{debug, info};

use crate::error::{DbError, DbResult};

/// Platform types for users
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Discord,
    Api,
    Cli,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Discord => write!(f, "discord"),
            Platform::Api => write!(f, "api"),
            Platform::Cli => write!(f, "cli"),
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "discord" => Ok(Platform::Discord),
            "api" => Ok(Platform::Api),
            "cli" => Ok(Platform::Cli),
            _ => Err(format!("Unknown platform: {}", s)),
        }
    }
}

/// User status types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Pending,
    Approved,
    Denied,
}

impl std::fmt::Display for UserStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserStatus::Pending => write!(f, "pending"),
            UserStatus::Approved => write!(f, "approved"),
            UserStatus::Denied => write!(f, "denied"),
        }
    }
}

impl std::str::FromStr for UserStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(UserStatus::Pending),
            "approved" => Ok(UserStatus::Approved),
            "denied" => Ok(UserStatus::Denied),
            _ => Err(format!("Unknown status: {}", s)),
        }
    }
}

/// User record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub status: UserStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub denied_at: Option<DateTime<Utc>>,
    pub welcomed: bool,
}

/// Convert SQLite timestamp (seconds since epoch) to DateTime<Utc>
fn from_timestamp(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

/// Convert DateTime<Utc> to SQLite timestamp
fn to_timestamp(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

/// User repository for database operations
pub struct UserRepository;

impl UserRepository {
    /// Get or create a user
    /// 
    /// If the user exists, returns the existing user.
    /// If not, creates a new user with pending status.
    pub async fn get_or_create(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        platform: Platform,
    ) -> DbResult<User> {
        // Try to get existing user
        if let Some(user) = Self::get_by_id(pool, id).await? {
            debug!("Found existing user: {}", id);
            return Ok(user);
        }

        // Create new user
        let now = Utc::now();
        let platform_str = platform.to_string();
        let status_str = UserStatus::Pending.to_string();

        sqlx::query(
            r#"
            INSERT INTO users (id, name, platform, status, created_at, updated_at, welcomed)
            VALUES (?, ?, ?, ?, ?, ?, 0)
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(&platform_str)
        .bind(&status_str)
        .bind(to_timestamp(now))
        .bind(to_timestamp(now))
        .execute(pool)
        .await?;

        // Log event
        Self::log_event(pool, id, "created", None).await?;

        info!("Created new user: {} (platform: {})", id, platform);

        // Return the created user
        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::UserNotFound(id.to_string()))
    }

    /// Get user by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
            FROM users
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    /// Check if a user is approved
    pub async fn is_approved(pool: &SqlitePool, id: &str) -> DbResult<bool> {
        let row = sqlx::query_as::<_, StatusRow>(
            "SELECT status FROM users WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.status == "approved").unwrap_or(false))
    }

    /// Approve a user
    pub async fn approve(pool: &SqlitePool, id: &str) -> DbResult<User> {
        let user = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::UserNotFound(id.to_string()))?;

        if user.status == UserStatus::Approved {
            return Ok(user); // Already approved
        }

        if user.status == UserStatus::Denied {
            return Err(DbError::InvalidTransition {
                from: "denied".to_string(),
                to: "approved".to_string(),
            });
        }

        let now = Utc::now();

        sqlx::query(
            r#"
            UPDATE users
            SET status = 'approved', updated_at = ?, approved_at = ?
            WHERE id = ?
            "#,
        )
        .bind(to_timestamp(now))
        .bind(to_timestamp(now))
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "approved", None).await?;

        info!("Approved user: {}", id);

        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::UserNotFound(id.to_string()))
    }

    /// Deny a user
    pub async fn deny(pool: &SqlitePool, id: &str) -> DbResult<User> {
        let user = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::UserNotFound(id.to_string()))?;

        if user.status == UserStatus::Denied {
            return Ok(user); // Already denied
        }

        if user.status == UserStatus::Approved {
            return Err(DbError::InvalidTransition {
                from: "approved".to_string(),
                to: "denied".to_string(),
            });
        }

        let now = Utc::now();

        sqlx::query(
            r#"
            UPDATE users
            SET status = 'denied', updated_at = ?, denied_at = ?
            WHERE id = ?
            "#,
        )
        .bind(to_timestamp(now))
        .bind(to_timestamp(now))
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "denied", None).await?;

        info!("Denied user: {}", id);

        Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| DbError::UserNotFound(id.to_string()))
    }

    /// Mark user as welcomed (typically for Discord users)
    pub async fn mark_welcomed(pool: &SqlitePool, id: &str) -> DbResult<()> {
        let now = Utc::now();

        sqlx::query(
            r#"
            UPDATE users
            SET welcomed = 1, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(to_timestamp(now))
        .bind(id)
        .execute(pool)
        .await?;

        Self::log_event(pool, id, "welcomed", None).await?;

        info!("Marked user as welcomed: {}", id);
        Ok(())
    }

    /// List users by status (and optionally by platform)
    pub async fn list_by_status(
        pool: &SqlitePool,
        status: UserStatus,
        platform: Option<Platform>,
    ) -> DbResult<Vec<User>> {
        let status_str = status.to_string();

        let rows = if let Some(platform) = platform {
            let platform_str = platform.to_string();
            sqlx::query_as::<_, UserRow>(
                r#"
                SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
                FROM users
                WHERE status = ? AND platform = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(&status_str)
            .bind(&platform_str)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, UserRow>(
                r#"
                SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
                FROM users
                WHERE status = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(&status_str)
            .fetch_all(pool)
            .await?
        };

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// List all users
    pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<User>> {
        let rows = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, name, platform, status, created_at, updated_at, approved_at, denied_at, welcomed
            FROM users
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Remove a user completely
    pub async fn remove(pool: &SqlitePool, id: &str) -> DbResult<()> {
        Self::log_event(pool, id, "removed", None).await?;

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;

        info!("Removed user: {}", id);
        Ok(())
    }

    /// Auto-prune pending users older than the specified hours
    pub async fn prune_pending(pool: &SqlitePool, hours: i64) -> DbResult<usize> {
        let cutoff = Utc::now() - chrono::TimeDelta::hours(hours);
        let cutoff_ts = to_timestamp(cutoff);

        let pending_ids: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT id
            FROM users
            WHERE status = 'pending' AND created_at < ?
            "#,
        )
        .bind(cutoff_ts)
        .fetch_all(pool)
        .await?;

        let event_data = serde_json::json!({
            "reason": "pruned",
            "cutoff_hours": hours,
            "cutoff_ts": cutoff_ts
        })
        .to_string();

        for (user_id, ) in &pending_ids {
            Self::log_event(pool, user_id, "removed", Some(&event_data)).await?;
        }

        let result = sqlx::query(
            r#"
            DELETE FROM users
            WHERE status = 'pending' AND created_at < ?
            "#,
        )
        .bind(cutoff_ts)
        .execute(pool)
        .await?;

        let count = result.rows_affected() as usize;
        if count > 0 {
            info!("Pruned {} pending users older than {} hours", count, hours);
        }

        Ok(count)
    }

    /// Log an event for audit trail
    async fn log_event(
        pool: &SqlitePool,
        user_id: &str,
        event_type: &str,
        event_data: Option<&str>,
    ) -> DbResult<()> {
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO user_events (user_id, event_type, event_data, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(event_type)
        .bind(event_data)
        .bind(to_timestamp(now))
        .execute(pool)
        .await?;

        Ok(())
    }
}

/// Internal row type for SQLx mapping
#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    name: String,
    platform: String,
    status: String,
    created_at: i64,
    updated_at: i64,
    approved_at: Option<i64>,
    denied_at: Option<i64>,
    welcomed: i64,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        User {
            id: row.id,
            name: row.name,
            platform: row.platform.parse().unwrap_or(Platform::Api),
            status: row.status.parse().unwrap_or(UserStatus::Pending),
            created_at: from_timestamp(row.created_at),
            updated_at: from_timestamp(row.updated_at),
            approved_at: row.approved_at.map(from_timestamp),
            denied_at: row.denied_at.map(from_timestamp),
            welcomed: row.welcomed != 0,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StatusRow {
    status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::create_test_pool;

    #[tokio::test]
    async fn test_get_or_create_user() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        // Create user
        let user = UserRepository::get_or_create(pool, "user1", "Test User", Platform::Discord)
            .await
            .unwrap();

        assert_eq!(user.id, "user1");
        assert_eq!(user.name, "Test User");
        assert_eq!(user.platform, Platform::Discord);
        assert_eq!(user.status, UserStatus::Pending);
        assert!(!user.welcomed);

        // Get existing user
        let user2 = UserRepository::get_or_create(pool, "user1", "Different Name", Platform::Discord)
            .await
            .unwrap();

        assert_eq!(user2.name, "Test User"); // Name should not change
    }

    #[tokio::test]
    async fn test_approve_user() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        // Create and approve user
        UserRepository::get_or_create(pool, "user1", "Test User", Platform::Discord)
            .await
            .unwrap();

        let user = UserRepository::approve(pool, "user1").await.unwrap();

        assert_eq!(user.status, UserStatus::Approved);
        assert!(user.approved_at.is_some());

        // Check is_approved
        assert!(UserRepository::is_approved(pool, "user1").await.unwrap());
    }

    #[tokio::test]
    async fn test_deny_user() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        UserRepository::get_or_create(pool, "user1", "Test User", Platform::Discord)
            .await
            .unwrap();

        let user = UserRepository::deny(pool, "user1").await.unwrap();

        assert_eq!(user.status, UserStatus::Denied);
        assert!(user.denied_at.is_some());

        // Cannot approve a denied user
        let result = UserRepository::approve(pool, "user1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_by_status() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        // Create users
        UserRepository::get_or_create(pool, "user1", "User 1", Platform::Discord)
            .await
            .unwrap();
        UserRepository::get_or_create(pool, "user2", "User 2", Platform::Discord)
            .await
            .unwrap();
        UserRepository::get_or_create(pool, "user3", "User 3", Platform::Api)
            .await
            .unwrap();

        // Approve one
        UserRepository::approve(pool, "user1").await.unwrap();

        // List pending
        let pending = UserRepository::list_by_status(pool, UserStatus::Pending, None)
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);

        // List approved
        let approved = UserRepository::list_by_status(pool, UserStatus::Approved, None)
            .await
            .unwrap();
        assert_eq!(approved.len(), 1);

        // List by platform
        let discord_pending = UserRepository::list_by_status(pool, UserStatus::Pending, Some(Platform::Discord))
            .await
            .unwrap();
        assert_eq!(discord_pending.len(), 1);
    }

    #[tokio::test]
    async fn test_prune_pending() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        // Create user
        UserRepository::get_or_create(pool, "user1", "User 1", Platform::Discord)
            .await
            .unwrap();

        // Manually set created_at to 2 hours ago
        let two_hours_ago = Utc::now() - chrono::TimeDelta::hours(2);
        sqlx::query("UPDATE users SET created_at = ? WHERE id = 'user1'")
            .bind(to_timestamp(two_hours_ago))
            .execute(pool)
            .await
            .unwrap();

        // Prune users older than 1 hour
        let pruned = UserRepository::prune_pending(pool, 1).await.unwrap();
        assert_eq!(pruned, 1);

        // User should be gone
        let user = UserRepository::get_by_id(pool, "user1").await.unwrap();
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn test_mark_welcomed() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        UserRepository::get_or_create(pool, "user1", "User 1", Platform::Discord)
            .await
            .unwrap();

        UserRepository::approve(pool, "user1").await.unwrap();
        UserRepository::mark_welcomed(pool, "user1").await.unwrap();

        let user = UserRepository::get_by_id(pool, "user1").await.unwrap().unwrap();
        assert!(user.welcomed);
    }
}
