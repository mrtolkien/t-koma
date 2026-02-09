//! Reflection job: curate conversation insights into structured knowledge.
//!
//! After heartbeat completes for a session, the reflection runner checks if
//! there are new messages since the last reflection. If so, it builds a prompt
//! with the full conversation transcript and sends it through `chat_job()` so
//! the ghost can create/update notes, curate references, and update identity
//! files without polluting the session history.

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::scheduler::JobKind;
use crate::state::{AppState, LogEntry};
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

    // Find the timestamp of the last successful reflection for this session.
    let last_reflection_ts = match JobLogRepository::latest_ok(
        pool,
        ghost_id,
        session_id,
        DbJobKind::Reflection,
    )
    .await
    {
        Ok(Some(log)) => log.finished_at.unwrap_or(log.started_at),
        Ok(None) => 0, // never reflected — process everything
        Err(_) => return,
    };

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

    // Build and run the reflection prompt
    let prompt = build_reflection_prompt(&recent_messages);

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
            let mut job_log = JobLog::start(ghost_id, DbJobKind::Reflection, session_id);
            job_log.transcript = job_result.transcript;
            job_log.finish(&status);

            if let Err(err) = JobLogRepository::insert(pool, &job_log).await {
                warn!("reflection: failed to write job log for {ghost_name}:{session_id}: {err}");
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
            let status = format!("error: {err}");
            let mut job_log = JobLog::start(ghost_id, DbJobKind::Reflection, session_id);
            job_log.finish(&status);

            let _ = JobLogRepository::insert(pool, &job_log).await;

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

/// Format recent messages as a full conversation transcript for reflection.
///
/// Includes complete tool use inputs and untruncated tool results so that
/// reflection has the same information the ghost had during the conversation.
fn format_messages(messages: &[t_koma_db::Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            MessageRole::Operator => "OPERATOR",
            MessageRole::Ghost => "GHOST",
        };
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    out.push_str(&format!("**{}**: {}\n\n", role, text));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    out.push_str(&format!(
                        "**{}** [tool_use: {} — {}]\n\n",
                        role, name, input
                    ));
                }
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    let label = if is_error == &Some(true) {
                        "tool_error"
                    } else {
                        "tool_result"
                    };
                    out.push_str(&format!("[{}: {}]\n\n", label, content));
                }
            }
        }
    }
    out
}

fn build_reflection_prompt(messages: &[t_koma_db::Message]) -> String {
    let recent_messages = format_messages(messages);

    crate::content::prompt_text(
        crate::content::ids::PROMPT_REFLECTION,
        None,
        &[("recent_messages", &recent_messages)],
    )
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to render reflection prompt: {e}, using fallback");
        format!(
            "You are in reflection mode. Process the following conversation into \
             structured knowledge.\n\n## Recent Conversation\n\n{recent_messages}"
        )
    })
}
