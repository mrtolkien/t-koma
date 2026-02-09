//! Session and message management for GHOST conversation history.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::{DbError, DbResult};

/// Message role types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    Operator,
    Ghost,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::Operator => write!(f, "operator"),
            MessageRole::Ghost => write!(f, "ghost"),
        }
    }
}

impl std::str::FromStr for MessageRole {
    type Err = DbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "operator" => Ok(MessageRole::Operator),
            "ghost" => Ok(MessageRole::Ghost),
            _ => Err(DbError::InvalidRole(s.to_string())),
        }
    }
}

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// A message in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    pub model: Option<String>,
    pub created_at: i64,
}

/// A session (conversation container)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub ghost_id: String,
    pub operator_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_active: bool,
    /// LLM-generated summary of compacted (older) messages.
    pub compaction_summary: Option<String>,
    /// ID of the last message included in the compaction summary.
    pub compaction_cursor_id: Option<String>,
}

/// Session info for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub is_active: bool,
}

/// Session repository for database operations
pub struct SessionRepository;

impl SessionRepository {
    /// Create a new session
    pub async fn create(pool: &SqlitePool, ghost_id: &str, operator_id: &str) -> DbResult<Session> {
        let id = format!("sess_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO sessions (id, ghost_id, operator_id, created_at, updated_at, is_active)
             VALUES (?, ?, ?, ?, ?, 1)",
        )
        .bind(&id)
        .bind(ghost_id)
        .bind(operator_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;

        sqlx::query(
            "UPDATE sessions
             SET is_active = 0
             WHERE ghost_id = ? AND operator_id = ? AND id != ?",
        )
        .bind(ghost_id)
        .bind(operator_id)
        .bind(&id)
        .execute(pool)
        .await?;

        info!("Created new session: {} for ghost: {}", id, ghost_id);

        Ok(Session {
            id,
            ghost_id: ghost_id.to_string(),
            operator_id: operator_id.to_string(),
            created_at: now,
            updated_at: now,
            is_active: true,
            compaction_summary: None,
            compaction_cursor_id: None,
        })
    }

    /// Get session by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> DbResult<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, ghost_id, operator_id, created_at, updated_at, is_active, compaction_summary, compaction_cursor_id
             FROM sessions
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Session::from))
    }

    /// Get session by ID with ghost_id verification
    pub async fn get_by_id_for_ghost(
        pool: &SqlitePool,
        id: &str,
        ghost_id: &str,
    ) -> DbResult<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, ghost_id, operator_id, created_at, updated_at, is_active, compaction_summary, compaction_cursor_id
             FROM sessions
             WHERE id = ? AND ghost_id = ?",
        )
        .bind(id)
        .bind(ghost_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Session::from))
    }

    /// Get active session for an operator within a specific ghost
    pub async fn get_active(
        pool: &SqlitePool,
        ghost_id: &str,
        operator_id: &str,
    ) -> DbResult<Option<Session>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, ghost_id, operator_id, created_at, updated_at, is_active, compaction_summary, compaction_cursor_id
             FROM sessions
             WHERE ghost_id = ? AND operator_id = ? AND is_active = 1",
        )
        .bind(ghost_id)
        .bind(operator_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Session::from))
    }

    /// Get or create active session for an operator within a ghost
    pub async fn get_or_create_active(
        pool: &SqlitePool,
        ghost_id: &str,
        operator_id: &str,
    ) -> DbResult<Session> {
        if let Some(session) = Self::get_active(pool, ghost_id, operator_id).await? {
            debug!(
                "Found active session: {} for ghost: {} operator: {}",
                session.id, ghost_id, operator_id
            );
            return Ok(session);
        }

        debug!(
            "No active session found for ghost: {} operator: {}, creating new",
            ghost_id, operator_id
        );
        Self::create(pool, ghost_id, operator_id).await
    }

    /// List all sessions for an operator within a ghost
    pub async fn list(
        pool: &SqlitePool,
        ghost_id: &str,
        operator_id: &str,
    ) -> DbResult<Vec<SessionInfo>> {
        let rows = sqlx::query_as::<_, SessionInfoRow>(
            "SELECT s.id, s.created_at, s.updated_at, s.is_active,
                    COUNT(m.id) as message_count
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             WHERE s.ghost_id = ? AND s.operator_id = ?
             GROUP BY s.id
             ORDER BY s.updated_at DESC",
        )
        .bind(ghost_id)
        .bind(operator_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(SessionInfo::from).collect())
    }

    /// List all sessions for a ghost (admin view, no operator filter).
    pub async fn list_for_ghost(pool: &SqlitePool, ghost_id: &str) -> DbResult<Vec<SessionInfo>> {
        let rows = sqlx::query_as::<_, SessionInfoRow>(
            "SELECT s.id, s.created_at, s.updated_at, s.is_active,
                    COUNT(m.id) as message_count
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             WHERE s.ghost_id = ?
             GROUP BY s.id
             ORDER BY s.updated_at DESC",
        )
        .bind(ghost_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(SessionInfo::from).collect())
    }

    /// List active sessions whose last update is at or before the given unix timestamp.
    pub async fn list_active_before(
        pool: &SqlitePool,
        before_unix_seconds: i64,
    ) -> DbResult<Vec<Session>> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, ghost_id, operator_id, created_at, updated_at, is_active, compaction_summary, compaction_cursor_id
             FROM sessions
             WHERE is_active = 1 AND updated_at <= ?
             ORDER BY updated_at ASC",
        )
        .bind(before_unix_seconds)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Session::from).collect())
    }

    /// Switch active session (within a ghost)
    pub async fn switch(
        pool: &SqlitePool,
        ghost_id: &str,
        operator_id: &str,
        session_id: &str,
    ) -> DbResult<Session> {
        let session = Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))?;

        if session.ghost_id != ghost_id || session.operator_id != operator_id {
            return Err(DbError::Unauthorized);
        }

        sqlx::query("UPDATE sessions SET is_active = 0 WHERE ghost_id = ? AND operator_id = ?")
            .bind(ghost_id)
            .bind(operator_id)
            .execute(pool)
            .await?;

        sqlx::query("UPDATE sessions SET is_active = 1 WHERE id = ?")
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
        ghost_id: &str,
        session_id: &str,
        role: MessageRole,
        content: Vec<ContentBlock>,
        model: Option<&str>,
    ) -> DbResult<Message> {
        let id = format!("msg_{}", Uuid::new_v4());
        let now = Utc::now().timestamp();
        let content_json =
            serde_json::to_string(&content).map_err(|e| DbError::Serialization(e.to_string()))?;

        sqlx::query(
            "INSERT INTO messages (id, ghost_id, session_id, role, content, model, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(ghost_id)
        .bind(session_id)
        .bind(role.to_string())
        .bind(&content_json)
        .bind(model)
        .bind(now)
        .execute(pool)
        .await?;

        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(session_id)
            .execute(pool)
            .await?;

        Ok(Message {
            id,
            session_id: session_id.to_string(),
            role,
            content,
            model: model.map(|m| m.to_string()),
            created_at: now,
        })
    }

    /// List messages for a session
    pub async fn list_messages(pool: &SqlitePool, session_id: &str) -> DbResult<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, model, created_at
             FROM messages
             WHERE session_id = ?
             ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(Message::try_from)
            .collect::<DbResult<Vec<_>>>()
    }

    /// Get the most recent message in a session (by created_at).
    pub async fn get_last_message(
        pool: &SqlitePool,
        session_id: &str,
    ) -> DbResult<Option<Message>> {
        let row = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, model, created_at
             FROM messages
             WHERE session_id = ?
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?;

        row.map(Message::try_from).transpose()
    }

    /// List all active sessions.
    pub async fn list_active(pool: &SqlitePool) -> DbResult<Vec<Session>> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, ghost_id, operator_id, created_at, updated_at, is_active, compaction_summary, compaction_cursor_id
             FROM sessions
             WHERE is_active = 1
             ORDER BY updated_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(Session::from).collect())
    }

    /// Delete a message by id.
    pub async fn delete_message(pool: &SqlitePool, message_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM messages WHERE id = ?")
            .bind(message_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Alias for list_messages (backwards-compatible)
    pub async fn get_messages(pool: &SqlitePool, session_id: &str) -> DbResult<Vec<Message>> {
        Self::list_messages(pool, session_id).await
    }

    /// Count messages in a session
    pub async fn count_messages(pool: &SqlitePool, session_id: &str) -> DbResult<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM messages WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
        Ok(row.try_get::<i64, _>("count").unwrap_or(0))
    }

    /// Get messages created after the given unix timestamp for a specific session.
    pub async fn get_messages_since(
        pool: &SqlitePool,
        session_id: &str,
        since_unix_seconds: i64,
    ) -> DbResult<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, model, created_at
             FROM messages
             WHERE session_id = ? AND created_at > ?
             ORDER BY created_at ASC",
        )
        .bind(session_id)
        .bind(since_unix_seconds)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(Message::try_from)
            .collect::<DbResult<Vec<_>>>()
    }

    /// Count messages created at or after the provided unix timestamp for a ghost.
    pub async fn count_messages_since(
        pool: &SqlitePool,
        ghost_id: &str,
        since_unix_seconds: i64,
    ) -> DbResult<i64> {
        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM messages WHERE ghost_id = ? AND created_at >= ?",
        )
        .bind(ghost_id)
        .bind(since_unix_seconds)
        .fetch_one(pool)
        .await?;
        Ok(row.try_get::<i64, _>("count").unwrap_or(0))
    }

    /// Delete a session if it belongs to the ghost and operator
    pub async fn delete(
        pool: &SqlitePool,
        ghost_id: &str,
        operator_id: &str,
        session_id: &str,
    ) -> DbResult<()> {
        let session = Self::get_by_id(pool, session_id)
            .await?
            .ok_or_else(|| DbError::SessionNotFound(session_id.to_string()))?;

        if session.ghost_id != ghost_id || session.operator_id != operator_id {
            return Err(DbError::Unauthorized);
        }

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get all tool uses in a session
    pub async fn get_tool_uses(
        pool: &SqlitePool,
        session_id: &str,
    ) -> DbResult<Vec<(String, serde_json::Value)>> {
        let messages = Self::list_messages(pool, session_id).await?;
        let mut tool_uses = Vec::new();

        for message in messages {
            for block in message.content {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    tool_uses.push((name, input));
                }
            }
        }

        Ok(tool_uses)
    }

    /// Get the last tool use in a session
    pub async fn get_last_tool_use(
        pool: &SqlitePool,
        session_id: &str,
    ) -> DbResult<Option<(String, serde_json::Value)>> {
        let tool_uses = Self::get_tool_uses(pool, session_id).await?;
        Ok(tool_uses.into_iter().last())
    }

    /// Persist compaction state for a session.
    pub async fn update_compaction(
        pool: &SqlitePool,
        session_id: &str,
        summary: &str,
        cursor_id: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE sessions SET compaction_summary = ?, compaction_cursor_id = ? WHERE id = ?",
        )
        .bind(summary)
        .bind(cursor_id)
        .bind(session_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Load messages after a compaction cursor (by created_at ordering).
    ///
    /// Returns messages whose `created_at` is strictly greater than the cursor
    /// message's `created_at`. This is used for compaction-aware history loading.
    pub async fn get_messages_after(
        pool: &SqlitePool,
        session_id: &str,
        cursor_id: &str,
    ) -> DbResult<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, role, content, model, created_at
             FROM messages
             WHERE session_id = ? AND created_at > (
                 SELECT created_at FROM messages WHERE id = ?
             )
             ORDER BY created_at ASC",
        )
        .bind(session_id)
        .bind(cursor_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(Message::try_from)
            .collect::<DbResult<Vec<_>>>()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    id: String,
    ghost_id: String,
    operator_id: String,
    created_at: i64,
    updated_at: i64,
    is_active: i64,
    compaction_summary: Option<String>,
    compaction_cursor_id: Option<String>,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Session {
            id: row.id,
            ghost_id: row.ghost_id,
            operator_id: row.operator_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            is_active: row.is_active != 0,
            compaction_summary: row.compaction_summary,
            compaction_cursor_id: row.compaction_cursor_id,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SessionInfoRow {
    id: String,
    created_at: i64,
    updated_at: i64,
    is_active: i64,
    message_count: i64,
}

impl From<SessionInfoRow> for SessionInfo {
    fn from(row: SessionInfoRow) -> Self {
        SessionInfo {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            message_count: row.message_count,
            is_active: row.is_active != 0,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
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

    fn try_from(row: MessageRow) -> Result<Self, Self::Error> {
        let content: Vec<ContentBlock> = serde_json::from_str(&row.content)
            .map_err(|e| DbError::Serialization(e.to_string()))?;

        Ok(Message {
            id: row.id,
            session_id: row.session_id,
            role: row.role.parse()?,
            content,
            model: row.model,
            created_at: row.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GhostRepository, OperatorAccessLevel, OperatorRepository, Platform,
        test_helpers::create_test_pool,
    };

    #[tokio::test]
    async fn test_session_lifecycle() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "TestOp",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let ghost = GhostRepository::create(pool, &operator.id, "TestGhost")
            .await
            .unwrap();

        let session = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();
        assert_eq!(session.ghost_id, ghost.id);
        assert_eq!(session.operator_id, operator.id);

        let active = SessionRepository::get_active(pool, &ghost.id, &operator.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(active.id, session.id);
    }

    #[tokio::test]
    async fn test_add_message() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "TestOp",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let ghost = GhostRepository::create(pool, &operator.id, "TestGhost")
            .await
            .unwrap();

        let session = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();

        let message = SessionRepository::add_message(
            pool,
            &ghost.id,
            &session.id,
            MessageRole::Operator,
            vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
            None,
        )
        .await
        .unwrap();

        assert_eq!(message.role, MessageRole::Operator);

        let messages = SessionRepository::list_messages(pool, &session.id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_count_messages_since() {
        let db = create_test_pool().await.unwrap();
        let pool = db.pool();

        let operator = OperatorRepository::create_new(
            pool,
            "TestOp",
            Platform::Api,
            OperatorAccessLevel::Standard,
        )
        .await
        .unwrap();
        let ghost = GhostRepository::create(pool, &operator.id, "TestGhost")
            .await
            .unwrap();

        let session = SessionRepository::create(pool, &ghost.id, &operator.id)
            .await
            .unwrap();
        SessionRepository::add_message(
            pool,
            &ghost.id,
            &session.id,
            MessageRole::Operator,
            vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
            None,
        )
        .await
        .unwrap();

        let count =
            SessionRepository::count_messages_since(pool, &ghost.id, Utc::now().timestamp() - 300)
                .await
                .unwrap();
        assert!(count >= 1);
    }
}
