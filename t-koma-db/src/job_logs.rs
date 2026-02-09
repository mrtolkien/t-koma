//! Job log storage for background tasks (heartbeat, reflection).
//!
//! Instead of writing every message to the session `messages` table,
//! background jobs collect their transcript in memory and persist a
//! single `job_logs` row when finished.

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

/// A completed job log row.
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
        }
    }

    /// Mark the job as finished with the given status.
    pub fn finish(&mut self, status: &str) {
        self.finished_at = Some(Utc::now().timestamp());
        self.status = Some(status.to_string());
    }
}

/// Repository for job_logs table operations.
pub struct JobLogRepository;

impl JobLogRepository {
    /// Insert a completed job log.
    pub async fn insert(pool: &SqlitePool, log: &JobLog) -> DbResult<()> {
        let transcript_json = serde_json::to_string(&log.transcript)
            .map_err(|e| DbError::Serialization(e.to_string()))?;

        sqlx::query(
            "INSERT INTO job_logs (id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&log.id)
        .bind(&log.ghost_id)
        .bind(log.job_kind.to_string())
        .bind(&log.session_id)
        .bind(log.started_at)
        .bind(log.finished_at)
        .bind(&log.status)
        .bind(&transcript_json)
        .execute(pool)
        .await?;

        Ok(())
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
            "SELECT id, ghost_id, job_kind, session_id, started_at, finished_at, status, transcript
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
}

impl TryFrom<JobLogRow> for JobLog {
    type Error = DbError;

    fn try_from(row: JobLogRow) -> Result<Self, Self::Error> {
        let job_kind: JobKind = row.job_kind.parse()?;
        let transcript: Vec<TranscriptEntry> = serde_json::from_str(&row.transcript)
            .map_err(|e| DbError::Serialization(e.to_string()))?;

        Ok(JobLog {
            id: row.id,
            ghost_id: row.ghost_id,
            job_kind,
            session_id: row.session_id,
            started_at: row.started_at,
            finished_at: row.finished_at,
            status: row.status,
            transcript,
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
}
