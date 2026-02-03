//! Session and message management for conversation history.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{debug, info};

use crate::error::{DbError, DbResult};

/// Convert SQLite timestamp (seconds since epoch) to DateTime<Utc>
fn from_timestamp(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

/// Convert DateTime<Utc> to SQLite timestamp
fn to_timestamp(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

/// A content block in a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse {
        id: String,
        name: String,
        #[serde(rename = "input")]
        input: serde_json::Value,
    },
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Role of a message
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
        }
    }
}

/// A message in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,  // JSON array of content blocks
    pub model: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A session (conversation container)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
}

/// Session info for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: i64,
    pub is_active: bool,
}

/// Session repository for database operations
pub struct SessionRepository;

impl SessionRepository {
    /// Create a new session
    pub async fn create(
        pool: &SqlitePool,
        user_id: &str,
        title: Option<&str>,
    ) -> DbResult<Session> {
        let id = format!("sess_{}", uuid::new_v4());
        let title = title.unwrap_or("New Session").to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO sessions (id, user_id, title, created_at, updated_at, is_active)
            VALUES (?, ?, ?, ?, ?, 1)
            "#,
        )
        .bind(&id)
        .bind(user_id)
        .bind(&title)
        .bind(to_timestamp(now))
        .bind(to_timestamp(now))
        .execute(pool)
        .await?;

        // Deactivate other sessions for this user
        sqlx::query(
            r#"
            UPDATE sessions
            SET is_active = 0
            WHERE user_id = ? AND id != ?
            "#,
        )
        .bind(user_id)
        .bind(&id)
        .execute(pool)
        .await?;

        info!("Created new session: {} for user: {}", id, user_id);

        Ok(Session {
            id,
            user_id: user_id.to_string(),
            title,
            created_at: now,
            updated_at: now,
            is_active: true,
        })
    }

    /// Get session by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            SELECT id, user_id, title, created_at, updated_at, is_active
            FROM sessions
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    /// Get active session for a user
    pub async fn get_active(pool: &SqlitePool, user_id: &str) -> DbResult<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"
            SELECT id, user_id, title, created_at, updated_at, is_active
            FROM sessions
            WHERE user_id = ? AND is_active = 1
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    /// Get or create active session for a user
    pub async fn get_or_create_active(
        pool: &SqlitePool,
        user_id: &str,
    ) -> DbResult<Session> {
        if let Some(session) = Self::get_active(pool, user_id).await? {
            debug!("Found active session: {} for user: {}", session.id, user_id);
            return Ok(session);
        }

        debug!("No active session found for user: {}, creating new", user_id);
        Self::create(pool, user_id, None).await
    }

    /// List all sessions for a user
    pub async fn list(pool: &SqlitePool, user_id: &str) -> DbResult<Vec<SessionInfo>> {
        let rows = sqlx::query_as::<_, SessionInfoRow>(
            r#"
            SELECT 
                s.id,
                s.title,
                s.created_at,
                s.updated_at,
                s.is_active,
                COUNT(m.id) as message_count
            FROM sessions s
            LEFT JOIN messages m ON s.id = m.session_id
            WHERE s.user_id = ?
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Switch active session
    pub async fn switch(pool: &SqlitePool, user_id: &str, session_id: &str) -> DbResult<Session> {
        // Verify session exists and belongs to user
        let session = Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))?;

        if session.user_id != user_id {
            return Err(DbError::Unauthorized);
        }

        // Deactivate all sessions for user
        sqlx::query(
            r#"
            UPDATE sessions
            SET is_active = 0
            WHERE user_id = ?
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        // Activate the specified session
        let now = Utc::now();
        sqlx::query(
            r#"
            UPDATE sessions
            SET is_active = 1, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(to_timestamp(now))
        .bind(session_id)
        .execute(pool)
        .await?;

        info!("Switched to session: {} for user: {}", session_id, user_id);

        // Return updated session
        Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))
    }

    /// Delete a session
    pub async fn delete(pool: &SqlitePool, user_id: &str, session_id: &str) -> DbResult<()> {
        // Verify session exists and belongs to user
        let session = Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))?;

        if session.user_id != user_id {
            return Err(DbError::Unauthorized);
        }

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(pool)
            .await?;

        info!("Deleted session: {} for user: {}", session_id, user_id);
        Ok(())
    }

    /// Update session title
    pub async fn update_title(
        pool: &SqlitePool,
        user_id: &str,
        session_id: &str,
        title: &str,
    ) -> DbResult<Session> {
        // Verify session exists and belongs to user
        let session = Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))?;

        if session.user_id != user_id {
            return Err(DbError::Unauthorized);
        }

        let now = Utc::now();
        sqlx::query(
            r#"
            UPDATE sessions
            SET title = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(title)
        .bind(to_timestamp(now))
        .bind(session_id)
        .execute(pool)
        .await?;

        Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))
    }

    /// Add a message to a session
    pub async fn add_message(
        pool: &SqlitePool,
        session_id: &str,
        role: MessageRole,
        content: Vec<ContentBlock>,
        model: Option<&str>,
    ) -> DbResult<Message> {
        let id = format!("msg_{}", uuid::new_v4());
        let now = Utc::now();
        let content_json = serde_json::to_string(&content)
            .map_err(|e| DbError::Serialization(e.to_string()))?;

        // Insert message
        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, role, content, model, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(role.to_string())
        .bind(&content_json)
        .bind(model)
        .bind(to_timestamp(now))
        .execute(pool)
        .await?;

        // Update session updated_at
        sqlx::query(
            r#"
            UPDATE sessions
            SET updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(to_timestamp(now))
        .bind(session_id)
        .execute(pool)
        .await?;

        debug!("Added message: {} to session: {}", id, session_id);

        Ok(Message {
            id,
            session_id: session_id.to_string(),
            role,
            content,
            model: model.map(|s| s.to_string()),
            created_at: now,
        })
    }

    /// Get all messages for a session
    pub async fn get_messages(pool: &SqlitePool, session_id: &str) -> DbResult<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, role, content, model, created_at
            FROM messages
            WHERE session_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(|r| r.try_into()).collect()
    }

    /// Get messages with limit (most recent N)
    pub async fn get_messages_with_limit(
        pool: &SqlitePool,
        session_id: &str,
        limit: usize,
    ) -> DbResult<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            r#"
            SELECT id, session_id, role, content, model, created_at
            FROM messages
            WHERE session_id = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        // Reverse to get chronological order
        let mut messages: Vec<Message> = rows
            .into_iter()
            .map(|r| r.try_into())
            .collect::<DbResult<_>>()?;
        messages.reverse();
        Ok(messages)
    }

    /// Count total messages in a session
    pub async fn count_messages(pool: &SqlitePool, session_id: &str) -> DbResult<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM messages
            WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .fetch_one(pool)
        .await?;

        Ok(count)
    }

    /// Get all tool uses for a session
    ///
    /// Returns a list of (tool_name, tool_input) tuples for all ToolUse content blocks
    /// in assistant messages, ordered chronologically.
    pub async fn get_tool_uses(
        pool: &SqlitePool,
        session_id: &str,
    ) -> DbResult<Vec<(String, serde_json::Value)>> {
        let messages = Self::get_messages(pool, session_id).await?;
        let mut tool_uses = Vec::new();

        for message in messages {
            if message.role == MessageRole::Assistant {
                for block in message.content {
                    if let ContentBlock::ToolUse { name, input, .. } = block {
                        tool_uses.push((name, input));
                    }
                }
            }
        }

        Ok(tool_uses)
    }

    /// Get the most recent tool use for a session
    ///
    /// Returns the last (tool_name, tool_input) tuple, or None if no tools were used.
    pub async fn get_last_tool_use(
        pool: &SqlitePool,
        session_id: &str,
    ) -> DbResult<Option<(String, serde_json::Value)>> {
        let tool_uses = Self::get_tool_uses(pool, session_id).await?;
        Ok(tool_uses.into_iter().last())
    }
}

// Internal row types for SQLx

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    user_id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
    is_active: i64,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Session {
            id: row.id,
            user_id: row.user_id,
            title: row.title,
            created_at: from_timestamp(row.created_at),
            updated_at: from_timestamp(row.updated_at),
            is_active: row.is_active != 0,
        }
    }
}

#[derive(sqlx::FromRow)]
struct SessionInfoRow {
    id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
    is_active: i64,
    message_count: i64,
}

impl From<SessionInfoRow> for SessionInfo {
    fn from(row: SessionInfoRow) -> Self {
        SessionInfo {
            id: row.id,
            title: row.title,
            created_at: from_timestamp(row.created_at),
            updated_at: from_timestamp(row.updated_at),
            message_count: row.message_count,
            is_active: row.is_active != 0,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: String,
    session_id: String,
    role: String,
    content: String,
    model: Option<String>,
    created_at: i64,
}

impl TryFrom<MessageRow> for Message {
    type Error = DbError;

    fn try_from(row: MessageRow) -> DbResult<Self> {
        let content: Vec<ContentBlock> = serde_json::from_str(&row.content)
            .map_err(|e| DbError::Serialization(e.to_string()))?;

        Ok(Message {
            id: row.id,
            session_id: row.session_id,
            role: row.role.parse().map_err(|_| DbError::InvalidRole(row.role))?,
            content,
            model: row.model,
            created_at: from_timestamp(row.created_at),
        })
    }
}

impl std::str::FromStr for MessageRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(MessageRole::User),
            "assistant" => Ok(MessageRole::Assistant),
            _ => Err(()),
        }
    }
}

// UUID generation helper
mod uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1);

    pub fn new_v4() -> String {
        format!("{:016x}", COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::create_test_pool;
    use crate::users::{Platform, UserRepository};

    async fn create_test_user(pool: &SqlitePool, user_id: &str) {
        UserRepository::get_or_create(pool, user_id, &format!("Test User {}", user_id), Platform::Api)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_create_session() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        create_test_user(pool, "user1").await;

        let session = SessionRepository::create(pool, "user1", Some("Test Session"))
            .await
            .unwrap();

        assert_eq!(session.user_id, "user1");
        assert_eq!(session.title, "Test Session");
        assert!(session.is_active);
    }

    #[tokio::test]
    async fn test_get_or_create_active() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        create_test_user(pool, "user1").await;
        create_test_user(pool, "user2").await;

        // Create first session
        let session1 = SessionRepository::get_or_create_active(pool, "user1")
            .await
            .unwrap();
        assert!(session1.is_active);

        // Should return same session
        let session2 = SessionRepository::get_or_create_active(pool, "user1")
            .await
            .unwrap();
        assert_eq!(session1.id, session2.id);

        // Different user should get different session
        let session3 = SessionRepository::get_or_create_active(pool, "user2")
            .await
            .unwrap();
        assert_ne!(session1.id, session3.id);
    }

    #[tokio::test]
    async fn test_add_and_get_messages() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        create_test_user(pool, "user1").await;

        let session = SessionRepository::create(pool, "user1", None)
            .await
            .unwrap();

        // Add text message
        let content = vec![ContentBlock::Text {
            text: "Hello!".to_string(),
        }];
        let msg = SessionRepository::add_message(
            pool,
            &session.id,
            MessageRole::User,
            content.clone(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);

        // Get messages
        let messages = SessionRepository::get_messages(pool, &session.id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_switch_session() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        create_test_user(pool, "user1").await;

        let session1 = SessionRepository::create(pool, "user1", Some("Session 1"))
            .await
            .unwrap();
        let session2 = SessionRepository::create(pool, "user1", Some("Session 2"))
            .await
            .unwrap();

        // session2 should be active (newest)
        let active = SessionRepository::get_active(pool, "user1").await.unwrap();
        assert_eq!(active.unwrap().id, session2.id);

        // Switch to session1
        let switched = SessionRepository::switch(pool, "user1", &session1.id)
            .await
            .unwrap();
        assert_eq!(switched.id, session1.id);
        assert!(switched.is_active);

        // Verify session2 is now inactive
        let s2 = SessionRepository::get_by_id(pool, &session2.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!s2.is_active);
    }

    #[tokio::test]
    async fn test_tool_use_content_block() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        create_test_user(pool, "user1").await;

        let session = SessionRepository::create(pool, "user1", None)
            .await
            .unwrap();

        // Add tool_use message
        let content = vec![
            ContentBlock::Text {
                text: "I'll check that.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tool_123".to_string(),
                name: "get_weather".to_string(),
                input: serde_json::json!({"location": "SF"}),
            },
        ];
        let msg = SessionRepository::add_message(
            pool,
            &session.id,
            MessageRole::Assistant,
            content,
            Some("claude-sonnet-4-5"),
        )
        .await
        .unwrap();

        assert_eq!(msg.content.len(), 2);
        assert_eq!(msg.model, Some("claude-sonnet-4-5".to_string()));

        // Verify round-trip
        let messages = SessionRepository::get_messages(pool, &session.id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.len(), 2);
    }
}
