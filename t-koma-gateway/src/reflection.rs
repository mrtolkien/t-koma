//! Reflection job: curate conversation insights into structured knowledge.
//!
//! After heartbeat completes for a session, the reflection runner checks if
//! there are new messages since the last reflection. If so, it builds a
//! filtered transcript and sends it through `chat_job()` with a dedicated
//! reflection tool manager and a `JobHandle` for real-time TODO persistence.

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::scheduler::JobKind;
use crate::state::{AppState, LogEntry};
use crate::tools::{JobHandle, ToolManager};
use t_koma_db::{
    ContentBlock, JobKind as DbJobKind, JobLog, JobLogRepository, MessageRole, SessionRepository,
};

/// Default reflection idle minutes (overridden by config).
const DEFAULT_REFLECTION_IDLE_MINUTES: i64 = 4;

/// Check whether reflection should run for a ghost and, if so, execute it.
///
/// Called from the heartbeat loop after a heartbeat tick completes for a session.
/// Conditions:
/// 1. New messages exist since last reflection
/// 2. Session has been idle for at least N minutes
#[allow(clippy::too_many_arguments)]
pub async fn maybe_run_reflection(
    state: &Arc<AppState>,
    ghost_name: &str,
    ghost_id: &str,
    session_id: &str,
    session_updated_at: i64,
    operator_id: &str,
    heartbeat_model_alias: Option<&str>,
    idle_minutes: Option<i64>,
) {
    run_reflection(
        state,
        ghost_name,
        ghost_id,
        session_id,
        session_updated_at,
        operator_id,
        heartbeat_model_alias,
        true,
        idle_minutes.unwrap_or(DEFAULT_REFLECTION_IDLE_MINUTES),
    )
    .await;
}

/// Run reflection immediately for a specific session.
///
/// Used by explicit operator actions (for example creating a new session), where
/// reflection should start right away for the previous session.
pub async fn run_reflection_now(
    state: &Arc<AppState>,
    ghost_name: &str,
    ghost_id: &str,
    session_id: &str,
    operator_id: &str,
    heartbeat_model_alias: Option<&str>,
) {
    run_reflection(
        state,
        ghost_name,
        ghost_id,
        session_id,
        Utc::now().timestamp(),
        operator_id,
        heartbeat_model_alias,
        false,
        0, // idle_minutes irrelevant when not enforced
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn run_reflection(
    state: &Arc<AppState>,
    ghost_name: &str,
    ghost_id: &str,
    session_id: &str,
    session_updated_at: i64,
    operator_id: &str,
    heartbeat_model_alias: Option<&str>,
    enforce_idle_gate: bool,
    idle_minutes: i64,
) {
    let now_ts = Utc::now().timestamp();
    let idle_secs = idle_minutes * 60;

    // Avoid reflection right after active chat messages.
    if enforce_idle_gate && now_ts - session_updated_at < idle_secs {
        return;
    }

    let pool = state.koma_db.pool();

    // Find the last successful reflection for handoff note + timestamp.
    let last_reflection = match JobLogRepository::latest_ok(
        pool,
        ghost_id,
        session_id,
        DbJobKind::Reflection,
    )
    .await
    {
        Ok(log) => log,
        Err(_) => return,
    };

    let last_reflection_ts = last_reflection
        .as_ref()
        .map(|log| log.finished_at.unwrap_or(log.started_at))
        .unwrap_or(0);

    let previous_handoff = last_reflection
        .as_ref()
        .and_then(|log| log.handoff_note.as_deref())
        .unwrap_or("(No previous handoff note — this is the first reflection run.)")
        .to_string();

    // Check if new messages exist since last reflection.
    let recent_messages =
        match SessionRepository::get_messages_since(pool, session_id, last_reflection_ts).await {
            Ok(msgs) if msgs.is_empty() => return,
            Ok(msgs) => msgs,
            Err(_) => return,
        };

    info!(
        "reflection: {} new messages since last reflection for ghost '{}'",
        recent_messages.len(),
        ghost_name
    );

    // INSERT job log early so TUI can see "in progress"
    let job_log = JobLog::start(ghost_id, DbJobKind::Reflection, session_id);
    let job_log_id = job_log.id.clone();

    if let Err(err) = JobLogRepository::insert_started(pool, &job_log).await {
        warn!("reflection: failed to insert started job log: {err}");
        return;
    }

    // Build reflection tool manager and job handle
    let reflection_tm = ToolManager::new_reflection(state.session_chat.skill_paths().to_vec());
    let job_handle = JobHandle::new(pool.clone(), job_log_id.clone());

    // Build the filtered transcript prompt
    let prompt = build_reflection_prompt(&recent_messages, &previous_handoff, ghost_name).await;

    let model = if let Some(alias) = heartbeat_model_alias {
        state
            .get_model_by_alias(alias)
            .unwrap_or_else(|| state.default_model())
    } else {
        state.default_model()
    };

    let chat_key = format!("{operator_id}:{ghost_name}:{session_id}");
    state.set_chat_in_flight(&chat_key).await;

    let result = state
        .session_chat
        .chat_job(
            &state.koma_db,
            ghost_id,
            model.client.as_ref(),
            &model.provider,
            &model.model,
            model.context_window,
            session_id,
            operator_id,
            &prompt,
            false, // recent messages are embedded in the prompt
            Some(&reflection_tm),
            Some(job_handle),
            Some(crate::session::REFLECTION_TOOL_LOOP_LIMIT),
        )
        .await;

    state.clear_chat_in_flight(&chat_key).await;

    // Update scheduler — no cooldown; reflection won't re-trigger until new messages appear
    let scheduler_key = format!("reflection:{ghost_name}");
    state
        .scheduler_set(JobKind::Reflection, &scheduler_key, None)
        .await;

    match result {
        Ok(job_result) => {
            let status = format!("processed {} messages", recent_messages.len());
            // Extract handoff note from the final response text
            let handoff_note = Some(job_result.response_text.as_str());

            if let Err(err) = JobLogRepository::finish(
                pool,
                &job_log_id,
                &status,
                &job_result.transcript,
                handoff_note,
            )
            .await
            {
                warn!("reflection: failed to finish job log for {ghost_name}:{session_id}: {err}");
            }

            state
                .log(LogEntry::Reflection {
                    ghost_name: ghost_name.to_string(),
                    session_id: session_id.to_string(),
                    status,
                })
                .await;
        }
        Err(err) => {
            warn!("reflection failed for ghost '{ghost_name}' session '{session_id}': {err:#}");

            let status = format!("error: {err:#}");

            // Extract partial transcript from ToolLoopLimitReached if available,
            // otherwise persist empty transcript.
            let partial_transcript = match &err {
                crate::session::ChatError::ToolLoopLimitReached(pending) => {
                    &pending.partial_transcript
                }
                _ => &job_log.transcript,
            };

            if let Err(e) =
                JobLogRepository::finish(pool, &job_log_id, &status, partial_transcript, None).await
            {
                warn!("reflection: failed to finish error job log: {e}");
            }

            state
                .log(LogEntry::Reflection {
                    ghost_name: ghost_name.to_string(),
                    session_id: session_id.to_string(),
                    status,
                })
                .await;
        }
    }
}

/// Format recent messages as a filtered conversation transcript for reflection.
///
/// Keeps text blocks from both roles. For tool use, emits a one-liner with
/// the tool name and first relevant argument (URL, query, etc.). Tool results
/// are stripped except for web_fetch status codes (helps reflection know which
/// fetches failed without searching for them).
fn format_chat_transcript(messages: &[t_koma_db::Message]) -> String {
    use std::collections::HashMap;

    // First pass: build tool_use_id → tool_name map
    let mut tool_names: HashMap<String, String> = HashMap::new();
    for msg in messages {
        for block in &msg.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                tool_names.insert(id.clone(), name.clone());
            }
        }
    }

    // Second pass: emit transcript with selective tool result metadata
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            MessageRole::Operator => "OPERATOR",
            MessageRole::Ghost => "GHOST",
        };
        let ts = chrono::DateTime::from_timestamp(msg.created_at, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| msg.created_at.to_string());
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    out.push_str(&format!("**{}** [{}]: {}\n\n", role, ts, text));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    let summary = extract_tool_summary(name, input);
                    out.push_str(&format!("→ Used {}({})\n\n", name, summary));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    let tool = tool_names
                        .get(tool_use_id)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    if let Some(annotation) = extract_result_annotation(tool, content) {
                        out.push_str(&format!("  ↳ {}\n\n", annotation));
                    }
                    // All other tool results are stripped
                }
            }
        }
    }
    out
}

/// Extract a brief annotation from a tool result for the filtered transcript.
///
/// Returns `Some(annotation)` only for tools where the result metadata is useful
/// to reflection (e.g., HTTP status for web_fetch). Returns `None` for everything
/// else — the full result is intentionally stripped to save tokens.
fn extract_result_annotation(tool_name: &str, content: &str) -> Option<String> {
    match tool_name {
        "web_fetch" => {
            // Content is `[Result #N] {"provider":...,"status":NNN,...}`
            let json_start = content.find('{')?;
            let parsed: serde_json::Value = serde_json::from_str(&content[json_start..]).ok()?;
            let status = parsed.get("status")?.as_u64()?;
            if !(200..300).contains(&status) {
                Some(format!("web_fetch returned status {}", status))
            } else {
                None // Don't annotate successful fetches — reflection already knows they're cached
            }
        }
        _ => None,
    }
}

/// Extract a concise summary from tool input for the transcript.
fn extract_tool_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "web_fetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("…")
            .to_string(),
        "web_search" => input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("…")
            .to_string(),
        "knowledge_search" => input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("…")
            .to_string(),
        "knowledge_get" => input
            .get("id")
            .or_else(|| input.get("topic"))
            .and_then(|v| v.as_str())
            .unwrap_or("…")
            .to_string(),
        "read_file" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("…")
            .to_string(),
        _ => {
            let s = input.to_string();
            if s.len() > 80 {
                format!("{}…", &s[..s.floor_char_boundary(80)])
            } else {
                s
            }
        }
    }
}

async fn build_reflection_prompt(
    messages: &[t_koma_db::Message],
    previous_handoff: &str,
    ghost_name: &str,
) -> String {
    let recent_messages = format_chat_transcript(messages);

    // Read today's diary file if it exists
    let diary_today = match t_koma_db::ghosts::ghost_workspace_path(ghost_name) {
        Ok(workspace) => {
            let today = Utc::now().format("%Y-%m-%d").to_string();
            let diary_path = workspace.join("diary").join(format!("{today}.md"));
            tokio::fs::read_to_string(&diary_path)
                .await
                .unwrap_or_default()
        }
        Err(_) => String::new(),
    };

    crate::content::prompt_text(
        crate::content::ids::PROMPT_REFLECTION,
        None,
        &[
            ("recent_messages", &recent_messages),
            ("previous_handoff", previous_handoff),
            ("diary_today", &diary_today),
        ],
    )
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to render reflection prompt: {e}, using fallback");
        format!(
            "You are in reflection mode. Process the following conversation into \
             structured knowledge.\n\n## Previous Handoff\n\n{previous_handoff}\n\n\
             ## Recent Conversation\n\n{recent_messages}"
        )
    })
}
