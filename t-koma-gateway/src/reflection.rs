//! Reflection job: curate inbox captures into structured knowledge.
//!
//! After heartbeat completes for a session, the reflection runner checks if
//! the ghost has unprocessed inbox files. If so, it renders the
//! `reflection-prompt.md` template (which includes note-guidelines.md) with
//! the inbox items, then sends it through `chat_job()` so the ghost can
//! create/update notes without polluting the session history.

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::scheduler::JobKind;
use crate::state::{AppState, LogEntry};
use t_koma_db::{JobKind as DbJobKind, JobLog, JobLogRepository};

const REFLECTION_COOLDOWN_SECS: i64 = 30 * 60; // 30 minutes

/// Check whether reflection should run for a ghost and, if so, execute it.
///
/// Called from the heartbeat loop after a heartbeat tick completes for a session.
/// Conditions:
/// 1. Ghost inbox has files
/// 2. Last reflection was > 30 minutes ago (tracked via scheduler)
pub async fn maybe_run_reflection(
    state: &Arc<AppState>,
    ghost_name: &str,
    session_id: &str,
    operator_id: &str,
    heartbeat_model_alias: Option<&str>,
) {
    let scheduler_key = format!("reflection:{ghost_name}");
    let now_ts = Utc::now().timestamp();

    // Check cooldown
    if let Some(next_due) = state
        .scheduler_get(JobKind::Reflection, &scheduler_key)
        .await
        && now_ts < next_due
    {
        return;
    }

    // Check if inbox has files
    let ghost_db = match state.get_or_init_ghost_db(ghost_name).await {
        Ok(db) => db,
        Err(_) => return,
    };
    let inbox_path = ghost_db.workspace_path().join("inbox");
    let inbox_items = match read_inbox_items(&inbox_path).await {
        Ok(items) if items.is_empty() => return,
        Ok(items) => items,
        Err(_) => return,
    };

    info!(
        "reflection: {} inbox items for ghost '{}'",
        inbox_items.len(),
        ghost_name
    );

    // Build and run the reflection prompt
    let prompt = build_reflection_prompt(&inbox_items);

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
            &ghost_db,
            &state.koma_db,
            model.client.as_ref(),
            &model.provider,
            &model.model,
            session_id,
            operator_id,
            &prompt,
            true, // load session history for reference context
        )
        .await;

    state.clear_chat_in_flight(&chat_key).await;

    // Update scheduler regardless of outcome
    let next_due = Utc::now().timestamp() + REFLECTION_COOLDOWN_SECS;
    state
        .scheduler_set(JobKind::Reflection, &scheduler_key, Some(next_due))
        .await;

    match result {
        Ok(job_result) => {
            let status = format!("processed {} items", inbox_items.len());
            let mut job_log = JobLog::start(DbJobKind::Reflection, session_id);
            job_log.transcript = job_result.transcript;
            job_log.finish(&status);

            if let Err(err) = JobLogRepository::insert(ghost_db.pool(), &job_log).await {
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
            let mut job_log = JobLog::start(DbJobKind::Reflection, session_id);
            job_log.finish(&status);

            let _ = JobLogRepository::insert(ghost_db.pool(), &job_log).await;

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

/// Read all inbox files, sorted chronologically by filename.
async fn read_inbox_items(inbox_path: &std::path::Path) -> std::io::Result<Vec<InboxItem>> {
    if !inbox_path.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(inbox_path).await?;
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let content = tokio::fs::read_to_string(&path).await?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        entries.push(InboxItem { filename, content });
    }

    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(entries)
}

struct InboxItem {
    filename: String,
    content: String,
}

fn build_reflection_prompt(items: &[InboxItem]) -> String {
    let mut inbox_text = String::new();
    for (i, item) in items.iter().enumerate() {
        inbox_text.push_str(&format!(
            "## Inbox Item {} — `{}`\n\n{}\n\n---\n\n",
            i + 1,
            item.filename,
            item.content,
        ));
    }

    crate::content::prompt_text(
        crate::content::ids::PROMPT_REFLECTION,
        None,
        &[("inbox_items", &inbox_text)],
    )
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to render reflection prompt: {e}, using fallback");
        format!(
            "You are in reflection mode. Process the following inbox items into \
             structured knowledge, then delete each inbox file.\n\n{inbox_text}"
        )
    })
}
