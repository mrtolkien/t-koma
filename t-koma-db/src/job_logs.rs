//! Job log storage for background tasks (heartbeat, reflection).
//!
//! Job lifecycle:
//! 1. `insert_started()` — INSERT at job start (TUI sees "in progress")
//! 2. `update_todos()` — UPDATE `todo_list` column mid-run (reflection observability)
//! 3. `finish()` — UPDATE `finished_at`, `status`, `transcript`, `handoff_note`
//!
//! The legacy `insert()` method persists a fully-populated row in one shot
//! (used by heartbeat which is short-lived and doesn't need mid-run visibility).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::{DbError, DbResult};
use crate::sessions::{ContentBlock, MessageRole};

/// The kind of background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Heartbeat,
    Reflection,
}

impl std::fmt::Display for JobKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobKind::Heartbeat => write!(f, "heartbeat"),
            JobKind::Reflection => write!(f, "reflection"),
        }
    }
}

impl std::str::FromStr for JobKind {
    type Err = DbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "heartbeat" => Ok(JobKind::Heartbeat),
            "reflection" => Ok(JobKind::Reflection),
            _ => Err(DbError::Serialization(format!("invalid job kind: {s}"))),
        }
    }
}

/// A single entry in a job transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Status of a single TODO item in a reflection job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Skipped,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Done => write!(f, "done"),
            TodoStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// A single item in a reflection job's TODO list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TodoStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A job log row.
#[derive(Debug, Clone)]
pub struct JobLog {
    pub id: String,
    pub ghost_id: String,
    pub job_kind: JobKind,
    pub session_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: Option<String>,
    pub transcript: Vec<TranscriptEntry>,
    pub todo_list: Vec<TodoItem>,
    pub handoff_note: Option<String>,
}

impl JobLog {
    /// Start building a new job log for the given ghost and session.
    pub fn start(ghost_id: &str, job_kind: JobKind, session_id: &str) -> Self {
        Self {
            id: format!("job_{}", Uuid::new_v4()),
            ghost_id: ghost_id.to_string(),
            job_kind,
            session_id: session_id.to_string(),
            started_at: Utc::now().timestamp(),
            finished_at: None,
            status: None,
            transcript: Vec::new(),
            todo_list: Vec::new(),
            handoff_note: None,
        }
    }

    /// Mark the job as finished with the given status.
    pub fn finish(&mut self, status: &str) {
        self.finished_at = Some(Utc::now().timestamp());
        self.status = Some(status.to_string());
    }
}

/// Lightweight job log summary without the full transcript.
///
/// Used for list views where loading the complete transcript JSON
/// would be wasteful.
#[derive(Debug, Clone)]
pub struct JobLogSummary {
    pub id: String,
    pub ghost_id: String,
    pub job_kind: JobKind,
    pub session_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: Option<String>,
    /// Last transcript entry's first text block (extracted via SQLite JSON1).
    pub last_message: Option<String>,
    pub todo_list: Vec<TodoItem>,
    pub handoff_note: Option<String>,
}

/// Repository for job_logs table operations.
pub struct JobLogRepository;

impl JobLogRepository {
    /// Insert a fully-populated job log in one shot.
    ///
    /// Used by short-lived jobs (heartbeat) that don't need mid-run observability.
    pub async fn insert(pool: &SqlitePool, log: &JobLog) -> DbResult<()> {
        let transcript_json = serde_json::to_string(&log.transcript)
            .map_err(|e| DbError::Serialization(e.to_string()))?;
        let todo_json = serialize_optional_json(&log.todo_list)?;

        sqlx::query(
            "INSERT INTO job_logs (id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript, todo_list, handoff_note)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.id)
        .bind(&log.ghost_id)
        .bind(log.job_kind.to_string())
        .bind(&log.session_id)
        .bind(log.started_at)
        .bind(log.finished_at)
        .bind(&log.status)
        .bind(&transcript_json)
        .bind(&todo_json)
        .bind(&log.handoff_note)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// INSERT a job log row at job start with minimal data.
    ///
    /// The row is visible immediately (TUI sees "in progress"). Call
    /// `update_todos()` mid-run and `finish()` when done.
    pub async fn insert_started(pool: &SqlitePool, log: &JobLog) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO job_logs (id, ghost_id, job_kind, session_id, started_at, transcript)
             VALUES (?, ?, ?, ?, ?, '[]')",
        )
        .bind(&log.id)
        .bind(&log.ghost_id)
        .bind(log.job_kind.to_string())
        .bind(&log.session_id)
        .bind(log.started_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// UPDATE the `todo_list` column mid-run for observability.
    pub async fn update_todos(pool: &SqlitePool, id: &str, todos: &[TodoItem]) -> DbResult<()> {
        let json =
            serde_json::to_string(todos).map_err(|e| DbError::Serialization(e.to_string()))?;

        sqlx::query("UPDATE job_logs SET todo_list = ? WHERE id = ?")
            .bind(&json)
            .bind(id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// UPDATE a started job log with final status, transcript, and handoff note.
    pub async fn finish(
        pool: &SqlitePool,
        id: &str,
        status: &str,
        transcript: &[TranscriptEntry],
        handoff_note: Option<&str>,
    ) -> DbResult<()> {
        let transcript_json =
            serde_json::to_string(transcript).map_err(|e| DbError::Serialization(e.to_string()))?;
        let finished_at = Utc::now().timestamp();

        sqlx::query(
            "UPDATE job_logs SET finished_at = ?, status = ?, transcript = ?, handoff_note = ? WHERE id = ?",
        )
        .bind(finished_at)
        .bind(status)
        .bind(&transcript_json)
        .bind(handoff_note)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get a single job log by ID (with full transcript).
    pub async fn get(pool: &SqlitePool, id: &str) -> DbResult<Option<JobLog>> {
        let row = sqlx::query_as::<_, JobLogRow>(
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript, todo_list, handoff_note
             FROM job_logs
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        row.map(JobLog::try_from).transpose()
    }

    /// List recent job logs across all ghosts (lightweight, no transcript).
    pub async fn list_recent(pool: &SqlitePool, limit: i64) -> DbResult<Vec<JobLogSummary>> {
        let rows = sqlx::query_as::<_, JobLogSummaryRow>(
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status,
                    json_extract(transcript, '$[#-1].content[0].text') as last_message,
                    todo_list, handoff_note
             FROM job_logs
             ORDER BY started_at DESC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(JobLogSummary::try_from)
            .collect::<DbResult<Vec<_>>>()
    }

    /// List recent job logs for a specific ghost (lightweight, no transcript).
    pub async fn list_for_ghost(
        pool: &SqlitePool,
        ghost_id: &str,
        limit: i64,
    ) -> DbResult<Vec<JobLogSummary>> {
        let rows = sqlx::query_as::<_, JobLogSummaryRow>(
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status,
                    json_extract(transcript, '$[#-1].content[0].text') as last_message,
                    todo_list, handoff_note
             FROM job_logs
             WHERE ghost_id = ?
             ORDER BY started_at DESC
             LIMIT ?",
        )
        .bind(ghost_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(JobLogSummary::try_from)
            .collect::<DbResult<Vec<_>>>()
    }

    /// Find the most recent successful job of the given kind since a timestamp.
    ///
    /// Used by heartbeat skip logic: "was there a successful heartbeat since
    /// the last session activity?"
    pub async fn latest_ok_since(
        pool: &SqlitePool,
        ghost_id: &str,
        session_id: &str,
        kind: JobKind,
        since_ts: i64,
    ) -> DbResult<Option<JobLog>> {
        let row = sqlx::query_as::<_, JobLogRow>(
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript, todo_list, handoff_note
             FROM job_logs
             WHERE ghost_id = ? AND session_id = ? AND job_kind = ? AND started_at >= ?
               AND status IS NOT NULL AND status NOT LIKE 'error:%'
             ORDER BY started_at DESC
             LIMIT 1",
        )
        .bind(ghost_id)
        .bind(session_id)
        .bind(kind.to_string())
        .bind(since_ts)
        .fetch_optional(pool)
        .await?;

        row.map(JobLog::try_from).transpose()
    }

    /// Find the most recent successful job of the given kind (no time bound).
    pub async fn latest_ok(
        pool: &SqlitePool,
        ghost_id: &str,
        session_id: &str,
        kind: JobKind,
    ) -> DbResult<Option<JobLog>> {
        let row = sqlx::query_as::<_, JobLogRow>(
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript, todo_list, handoff_note
             FROM job_logs
             WHERE ghost_id = ? AND session_id = ? AND job_kind = ?
               AND status IS NOT NULL AND status NOT LIKE 'error:%'
             ORDER BY started_at DESC
             LIMIT 1",
        )
        .bind(ghost_id)
        .bind(session_id)
        .bind(kind.to_string())
        .fetch_optional(pool)
        .await?;

        row.map(JobLog::try_from).transpose()
    }
}

#[derive(Debug, sqlx::FromRow)]
struct JobLogRow {
    id: String,
    ghost_id: String,
    job_kind: String,
    session_id: String,
    started_at: i64,
    finished_at: Option<i64>,
    status: Option<String>,
    transcript: String,
    todo_list: Option<String>,
    handoff_note: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct JobLogSummaryRow {
    id: String,
    ghost_id: String,
    job_kind: String,
    session_id: String,
    started_at: i64,
    finished_at: Option<i64>,
    status: Option<String>,
    last_message: Option<String>,
    todo_list: Option<String>,
    handoff_note: Option<String>,
}

fn parse_optional_json<T: serde::de::DeserializeOwned>(json: Option<&str>) -> Vec<T> {
    json.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

fn serialize_optional_json<T: serde::Serialize>(items: &[T]) -> DbResult<Option<String>> {
    if items.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(items)
            .map(Some)
            .map_err(|e| DbError::Serialization(e.to_string()))
    }
}

impl TryFrom<JobLogSummaryRow> for JobLogSummary {
    type Error = DbError;

    fn try_from(row: JobLogSummaryRow) -> Result<Self, Self::Error> {
        let job_kind: JobKind = row.job_kind.parse()?;
        let todo_list = parse_optional_json(row.todo_list.as_deref());
        Ok(JobLogSummary {
            id: row.id,
            ghost_id: row.ghost_id,
            job_kind,
            session_id: row.session_id,
            started_at: row.started_at,
            finished_at: row.finished_at,
            status: row.status,
            last_message: row.last_message,
            todo_list,
            handoff_note: row.handoff_note,
        })
    }
}

impl TryFrom<JobLogRow> for JobLog {
    type Error = DbError;

    fn try_from(row: JobLogRow) -> Result<Self, Self::Error> {
        let job_kind: JobKind = row.job_kind.parse()?;
        let transcript: Vec<TranscriptEntry> = serde_json::from_str(&row.transcript)
            .map_err(|e| DbError::Serialization(e.to_string()))?;
        let todo_list = parse_optional_json(row.todo_list.as_deref());

        Ok(JobLog {
            id: row.id,
            ghost_id: row.ghost_id,
            job_kind,
            session_id: row.session_id,
            started_at: row.started_at,
            finished_at: row.finished_at,
            status: row.status,
            transcript,
            todo_list,
            handoff_note: row.handoff_note,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GhostRepository, OperatorAccessLevel, OperatorRepository, Platform,
        sessions::SessionRepository, test_helpers::create_test_pool,
    };

    #[tokio::test]
    async fn test_insert_and_retrieve_job_log() {
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

        let mut log = JobLog::start(&ghost.id, JobKind::Heartbeat, &session.id);
        log.transcript.push(TranscriptEntry {
            role: MessageRole::Operator,
            content: vec![ContentBlock::Text {
                text: "heartbeat prompt".to_string(),
            }],
            model: None,
        });
        log.transcript.push(TranscriptEntry {
            role: MessageRole::Ghost,
            content: vec![ContentBlock::Text {
                text: "HEARTBEAT_OK".to_string(),
            }],
            model: Some("test-model".to_string()),
        });
        log.finish("ok");

        JobLogRepository::insert(pool, &log).await.unwrap();

        let found = JobLogRepository::latest_ok_since(
            pool,
            &ghost.id,
            &session.id,
            JobKind::Heartbeat,
            log.started_at - 1,
        )
        .await
        .unwrap();

        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.job_kind, JobKind::Heartbeat);
        assert_eq!(found.status.as_deref(), Some("ok"));
        assert_eq!(found.transcript.len(), 2);
    }

    #[tokio::test]
    async fn test_latest_ok_since_ignores_errors() {
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

        let mut log = JobLog::start(&ghost.id, JobKind::Heartbeat, &session.id);
        log.finish("error: something went wrong");
        JobLogRepository::insert(pool, &log).await.unwrap();

        let found = JobLogRepository::latest_ok_since(
            pool,
            &ghost.id,
            &session.id,
            JobKind::Heartbeat,
            log.started_at - 1,
        )
        .await
        .unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_insert_started_and_finish() {
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

        let log = JobLog::start(&ghost.id, JobKind::Reflection, &session.id);
        let log_id = log.id.clone();
        JobLogRepository::insert_started(pool, &log).await.unwrap();

        // Visible immediately with no status
        let fetched = JobLogRepository::get(pool, &log_id).await.unwrap().unwrap();
        assert!(fetched.status.is_none());
        assert!(fetched.finished_at.is_none());

        // Update todos mid-run
        let todos = vec![TodoItem {
            title: "Review conversation".to_string(),
            description: None,
            status: TodoStatus::InProgress,
            note: None,
        }];
        JobLogRepository::update_todos(pool, &log_id, &todos)
            .await
            .unwrap();

        let fetched = JobLogRepository::get(pool, &log_id).await.unwrap().unwrap();
        assert_eq!(fetched.todo_list.len(), 1);
        assert_eq!(fetched.todo_list[0].status, TodoStatus::InProgress);

        // Finish with transcript and handoff
        let transcript = vec![TranscriptEntry {
            role: MessageRole::Ghost,
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            model: None,
        }];
        JobLogRepository::finish(
            pool,
            &log_id,
            "ok",
            &transcript,
            Some("Next: curate references"),
        )
        .await
        .unwrap();

        let fetched = JobLogRepository::get(pool, &log_id).await.unwrap().unwrap();
        assert_eq!(fetched.status.as_deref(), Some("ok"));
        assert!(fetched.finished_at.is_some());
        assert_eq!(fetched.transcript.len(), 1);
        assert_eq!(
            fetched.handoff_note.as_deref(),
            Some("Next: curate references")
        );
    }

    #[tokio::test]
    async fn test_transcript_json_round_trip() {
        let entries = vec![
            TranscriptEntry {
                role: MessageRole::Operator,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                model: None,
            },
            TranscriptEntry {
                role: MessageRole::Ghost,
                content: vec![
                    ContentBlock::Text {
                        text: "thinking...".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tu_1".to_string(),
                        name: "search".to_string(),
                        input: serde_json::json!({"q": "test"}),
                    },
                ],
                model: Some("model-1".to_string()),
            },
            TranscriptEntry {
                role: MessageRole::Operator,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".to_string(),
                    content: "found it".to_string(),
                    is_error: None,
                }],
                model: None,
            },
        ];

        let json = serde_json::to_string(&entries).unwrap();
        let parsed: Vec<TranscriptEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[1].content.len(), 2);
    }

    #[tokio::test]
    async fn test_todo_item_round_trip() {
        let items = vec![
            TodoItem {
                title: "Save reference".to_string(),
                description: Some("From web_fetch result".to_string()),
                status: TodoStatus::Done,
                note: Some("Saved to dioxus topic".to_string()),
            },
            TodoItem {
                title: "Update diary".to_string(),
                description: None,
                status: TodoStatus::Pending,
                note: None,
            },
        ];

        let json = serde_json::to_string(&items).unwrap();
        let parsed: Vec<TodoItem> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].status, TodoStatus::Done);
        assert_eq!(parsed[1].status, TodoStatus::Pending);
    }
}
