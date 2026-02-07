use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use tokio::fs;
use tokio::time::{Instant, interval_at};
use tracing::{info, warn};

use crate::session::ChatError;
use crate::state::{AppState, HeartbeatOverride, LogEntry};
use t_koma_db::{ContentBlock, GhostRepository, Message, Session, SessionRepository};

const HEARTBEAT_TOKEN: &str = "HEARTBEAT_OK";
const HEARTBEAT_CONTINUE_TOKEN: &str = "HEARTBEAT_CONTINUE";
const HEARTBEAT_IDLE_MINUTES: i64 = 15;
const HEARTBEAT_CHECK_SECONDS: u64 = 60;
const DEFAULT_HEARTBEAT_ACK_MAX_CHARS: usize = 300;

const DEFAULT_HEARTBEAT_PROMPT: &str = "Read HEARTBEAT.md if it exists (workspace context). Follow it strictly. \
Do not infer or repeat old tasks from prior chats. If nothing needs attention, reply HEARTBEAT_OK. \
Also review the inbox and promote items into notes.";

const HEARTBEAT_TEMPLATE: &str = include_str!("../prompts/heartbeat-template.md");

struct StripResult {
    should_skip: bool,
}

fn strip_token_at_edges(raw: &str) -> (String, bool) {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return (String::new(), false);
    }

    if !text.contains(HEARTBEAT_TOKEN) {
        return (text, false);
    }

    let mut did_strip = false;
    let mut changed = true;
    while changed {
        changed = false;
        let next = text.trim();
        if let Some(after) = next.strip_prefix(HEARTBEAT_TOKEN) {
            let after = after.trim_start();
            text = after.to_string();
            did_strip = true;
            changed = true;
            continue;
        }
        if next.ends_with(HEARTBEAT_TOKEN) {
            let before = &next[..next.len().saturating_sub(HEARTBEAT_TOKEN.len())];
            text = before.trim_end().to_string();
            did_strip = true;
            changed = true;
        }
    }

    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (collapsed, did_strip)
}

fn strip_heartbeat_token(raw: &str, max_ack_chars: usize) -> StripResult {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return StripResult { should_skip: true };
    }

    let strip_markup = |text: &str| {
        let mut out = String::with_capacity(text.len());
        let mut in_tag = false;
        for ch in text.chars() {
            if ch == '<' {
                in_tag = true;
                out.push(' ');
                continue;
            }
            if ch == '>' {
                in_tag = false;
                out.push(' ');
                continue;
            }
            if !in_tag {
                out.push(ch);
            }
        }
        out = out.replace("&nbsp;", " ");
        out.trim_matches(|c: char| "*`~_".contains(c)).to_string()
    };

    let normalized = strip_markup(trimmed);
    let has_token = trimmed.contains(HEARTBEAT_TOKEN) || normalized.contains(HEARTBEAT_TOKEN);
    if !has_token {
        return StripResult { should_skip: false };
    }

    let (stripped_original, did_strip_original) = strip_token_at_edges(trimmed);
    let (stripped_normalized, did_strip_normalized) = strip_token_at_edges(&normalized);
    let (picked, did_strip) = if did_strip_original && !stripped_original.is_empty() {
        (stripped_original, did_strip_original)
    } else {
        (stripped_normalized, did_strip_normalized)
    };

    if !did_strip {
        return StripResult { should_skip: false };
    }

    if picked.is_empty() {
        return StripResult { should_skip: true };
    }

    if picked.len() <= max_ack_chars {
        return StripResult { should_skip: true };
    }

    StripResult { should_skip: false }
}

fn strip_front_matter(raw: &str) -> &str {
    if !raw.starts_with("---") {
        return raw;
    }
    if let Some(end) = raw.find("\n---") {
        let start = end + "\n---".len();
        return raw[start..].trim_start();
    }
    raw
}

pub fn ensure_heartbeat_file(workspace_path: &Path) -> std::io::Result<()> {
    let path = workspace_path.join("HEARTBEAT.md");
    if path.exists() {
        return Ok(());
    }
    let content = strip_front_matter(HEARTBEAT_TEMPLATE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    use std::io::Write;
    file.write_all(content.as_bytes())?;
    Ok(())
}

fn is_heartbeat_continue(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = {
        let mut out = String::with_capacity(trimmed.len());
        let mut in_tag = false;
        for ch in trimmed.chars() {
            if ch == '<' {
                in_tag = true;
                out.push(' ');
                continue;
            }
            if ch == '>' {
                in_tag = false;
                out.push(' ');
                continue;
            }
            if !in_tag {
                out.push(ch);
            }
        }
        out.replace("&nbsp;", " ")
            .trim_matches(|c: char| "*`~_".contains(c))
            .trim()
            .to_string()
    };

    normalized == HEARTBEAT_CONTINUE_TOKEN
}

fn extract_message_text(message: &Message) -> String {
    let mut parts = Vec::new();
    for block in &message.content {
        if let ContentBlock::Text { text } = block
            && !text.trim().is_empty()
        {
            parts.push(text.trim());
        }
    }
    parts.join("\n")
}

fn last_message_is_heartbeat_ok(message: &Message) -> bool {
    let text = extract_message_text(message);
    if text.trim().is_empty() {
        return false;
    }
    let stripped = strip_heartbeat_token(&text, DEFAULT_HEARTBEAT_ACK_MAX_CHARS);
    stripped.should_skip
}

pub fn next_heartbeat_due_for_session(
    session_updated_at: i64,
    last_message: Option<&Message>,
    override_entry: Option<HeartbeatOverride>,
) -> Option<i64> {
    if let Some(override_entry) = override_entry {
        return Some(override_entry.next_due);
    }
    if let Some(message) = last_message
        && last_message_is_heartbeat_ok(message)
    {
        return None;
    }
    Some(session_updated_at + HEARTBEAT_IDLE_MINUTES * 60)
}

fn is_heartbeat_content_effectively_empty(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            let after_hashes = trimmed.trim_start_matches('#');
            if after_hashes.is_empty() || after_hashes.starts_with(' ') {
                continue;
            }
        }
        if trimmed.starts_with('-') || trimmed.starts_with('*') || trimmed.starts_with('+') {
            let tail = trimmed[1..].trim_start();
            if tail.is_empty() || tail == "[ ]" || tail == "[x]" || tail == "[X]" {
                continue;
            }
        }
        return false;
    }
    true
}

async fn should_skip_empty_heartbeat_file(workspace_path: &Path) -> bool {
    let heartbeat_path = workspace_path.join("HEARTBEAT.md");
    match fs::read_to_string(&heartbeat_path).await {
        Ok(content) => is_heartbeat_content_effectively_empty(&content),
        Err(_) => false,
    }
}

async fn run_heartbeat_for_session(
    state: &AppState,
    ghost_name: &str,
    session: &Session,
    heartbeat_model_alias: Option<&str>,
) -> Result<String, ChatError> {
    let prompt = DEFAULT_HEARTBEAT_PROMPT;

    let model = if let Some(alias) = heartbeat_model_alias {
        state
            .get_model_by_alias(alias)
            .unwrap_or_else(|| state.default_model())
    } else {
        state.default_model()
    };

    let ghost_db = state.get_or_init_ghost_db(ghost_name).await?;

    let response = state
        .session_chat
        .chat(
            &ghost_db,
            &state.koma_db,
            model.client.as_ref(),
            &model.provider,
            &model.model,
            &session.id,
            &session.operator_id,
            prompt,
        )
        .await?;

    Ok(response)
}

pub async fn run_heartbeat_tick(state: Arc<AppState>, heartbeat_model_alias: Option<String>) {
    let threshold = Utc::now() - ChronoDuration::minutes(HEARTBEAT_IDLE_MINUTES);
    let threshold_ts = threshold.timestamp();
    let now_ts = Utc::now().timestamp();

    let ghosts = match GhostRepository::list_all(state.koma_db.pool()).await {
        Ok(list) => list,
        Err(err) => {
            warn!("heartbeat: failed to list ghosts: {err}");
            return;
        }
    };

    for ghost in ghosts {
        let ghost_db = match state.get_or_init_ghost_db(&ghost.name).await {
            Ok(db) => db,
            Err(err) => {
                warn!("heartbeat: failed to init ghost db {}: {err}", ghost.name);
                continue;
            }
        };

        let sessions = match SessionRepository::list_active(ghost_db.pool()).await {
            Ok(list) => list,
            Err(err) => {
                warn!(
                    "heartbeat: failed to list sessions for {}: {err}",
                    ghost.name
                );
                continue;
            }
        };

        for session in sessions {
            let chat_key = format!("{}:{}:{}", session.operator_id, ghost.name, session.id);
            if state.is_chat_in_flight(&chat_key).await {
                continue;
            }

            let mut override_entry = state.get_heartbeat_override(&chat_key).await;
            if let Some(entry) = override_entry
                && session.updated_at > entry.last_seen_updated_at
            {
                state.clear_heartbeat_override(&chat_key).await;
                override_entry = None;
            }

            let last_message =
                match SessionRepository::get_last_message(ghost_db.pool(), &session.id).await {
                    Ok(msg) => msg,
                    Err(err) => {
                        warn!(
                            "heartbeat: failed to load last message for {}:{}: {err}",
                            ghost.name, session.id
                        );
                        continue;
                    }
                };

            let next_due = next_heartbeat_due_for_session(
                session.updated_at,
                last_message.as_ref(),
                override_entry,
            );
            state.set_heartbeat_due(&chat_key, next_due).await;

            if let Some(entry) = override_entry
                && now_ts < entry.next_due
            {
                continue;
            }

            if override_entry.is_none() && session.updated_at > threshold_ts {
                continue;
            }

            if override_entry.is_none()
                && let Some(message) = &last_message
                && last_message_is_heartbeat_ok(message)
            {
                continue;
            }

            if should_skip_empty_heartbeat_file(ghost_db.workspace_path()).await {
                continue;
            }

            let model_alias = heartbeat_model_alias.as_deref();
            let result =
                run_heartbeat_for_session(state.as_ref(), &ghost.name, &session, model_alias).await;

            match result {
                Ok(text) => {
                    if is_heartbeat_continue(&text) {
                        let last_seen_updated_at = Utc::now().timestamp();
                        let next_due = last_seen_updated_at + 30 * 60;
                        state
                            .set_heartbeat_override(&chat_key, next_due, last_seen_updated_at)
                            .await;

                        if let Ok(Some(last)) =
                            SessionRepository::get_last_message(ghost_db.pool(), &session.id).await
                            && matches!(last.role, t_koma_db::MessageRole::Ghost)
                        {
                            let last_text = extract_message_text(&last);
                            if is_heartbeat_continue(&last_text) {
                                let _ =
                                    SessionRepository::delete_message(ghost_db.pool(), &last.id)
                                        .await;
                            }
                        }

                        state
                            .log(LogEntry::Heartbeat {
                                ghost_name: ghost.name.clone(),
                                session_id: session.id.clone(),
                                status: "continue".to_string(),
                            })
                            .await;
                    } else {
                        state
                            .log(LogEntry::Heartbeat {
                                ghost_name: ghost.name.clone(),
                                session_id: session.id.clone(),
                                status: "ran".to_string(),
                            })
                            .await;
                    }
                }
                Err(err) => {
                    state
                        .log(LogEntry::Heartbeat {
                            ghost_name: ghost.name.clone(),
                            session_id: session.id.clone(),
                            status: format!("error: {err}"),
                        })
                        .await;
                }
            }
        }
    }
}

pub fn start_heartbeat_runner(
    state: Arc<AppState>,
    heartbeat_model_alias: Option<String>,
) -> tokio::task::JoinHandle<()> {
    let mut interval = interval_at(
        Instant::now() + Duration::from_secs(HEARTBEAT_CHECK_SECONDS),
        Duration::from_secs(HEARTBEAT_CHECK_SECONDS),
    );

    let handle = tokio::spawn(async move {
        loop {
            interval.tick().await;
            run_heartbeat_tick(Arc::clone(&state), heartbeat_model_alias.clone()).await;
        }
    });

    info!(
        "heartbeat runner started (idle_minutes={}, check_seconds={})",
        HEARTBEAT_IDLE_MINUTES, HEARTBEAT_CHECK_SECONDS
    );
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_empty_detection() {
        let content = "# Header\n\n- [ ]\n\n";
        assert!(is_heartbeat_content_effectively_empty(content));

        let content = "# Header\n\n- [ ] Task\n";
        assert!(!is_heartbeat_content_effectively_empty(content));
    }

    #[test]
    fn strip_heartbeat_ack() {
        let stripped = strip_heartbeat_token("HEARTBEAT_OK", 300);
        assert!(stripped.should_skip);

        let stripped = strip_heartbeat_token("**HEARTBEAT_OK**", 300);
        assert!(stripped.should_skip);

        let stripped = strip_heartbeat_token("HEARTBEAT_OK ok", 5);
        assert!(stripped.should_skip);

        let stripped = strip_heartbeat_token("HEARTBEAT_OK ok", 1);
        assert!(!stripped.should_skip);
    }

    #[test]
    fn heartbeat_continue_detection() {
        assert!(is_heartbeat_continue("HEARTBEAT_CONTINUE"));
        assert!(is_heartbeat_continue("**HEARTBEAT_CONTINUE**"));
        assert!(!is_heartbeat_continue("HEARTBEAT_CONTINUE later"));
    }
}
