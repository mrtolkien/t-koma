use std::path::PathBuf;
use std::sync::Arc;

use chrono::NaiveDate;
use tracing::{info, warn};

use crate::chat::compaction::{CompactionConfig, compact_if_needed, mask_tool_results};
use crate::chat::history::{
    ChatContentBlock, ChatMessage, ChatRole, build_history_messages, build_transcript_messages,
};
use crate::chat::prompt_cache::{PromptCacheManager, hash_context};
use crate::prompt::SystemPrompt;
use crate::prompt::render::{SystemBlock, build_system_prompt};
use crate::providers::provider::{
    Provider, ProviderContentBlock, ProviderError, ProviderResponse, extract_all_text,
    has_tool_uses,
};
use crate::state::ToolCallSummary;
use crate::system_info;
use crate::tools::context::{ApprovalReason, is_within_workspace};
use crate::tools::{JobHandle, ToolContext, ToolManager};
use serde_json::Value;
use t_koma_db::{
    ContentBlock as DbContentBlock, GhostRepository, KomaDbPool, MessageRole, OperatorRepository,
    Session, SessionRepository, TokenUsage, TranscriptEntry, UsageLog, UsageLogRepository,
    ghosts::ghost_workspace_path,
};

/// Errors that can occur during session chat
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("Database error: {0}")]
    Database(#[from] t_koma_db::DbError),

    #[error("Provider error: {0}")]
    Provider(ProviderError),

    #[error("Session not found or access denied")]
    SessionNotFound,

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Tool approval required")]
    ToolApprovalRequired(PendingToolApproval),

    #[error("Tool loop limit reached")]
    ToolLoopLimitReached(PendingToolContinuation),

    #[error("Provider returned an empty final response")]
    EmptyResponse,

    #[error("All models in fallback chain exhausted")]
    AllModelsExhausted,
}

#[derive(Debug, Clone)]
pub struct PendingToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct PendingToolApproval {
    pub pending_tool_uses: Vec<PendingToolUse>,
    pub completed_results: Vec<DbContentBlock>,
    pub reason: ApprovalReason,
}

#[derive(Debug, Clone)]
pub struct PendingToolContinuation {
    pub pending_tool_uses: Vec<PendingToolUse>,
    /// Partial transcript accumulated before hitting the loop limit.
    pub partial_transcript: Vec<TranscriptEntry>,
}

pub const DEFAULT_TOOL_LOOP_LIMIT: usize = 10;
pub const DEFAULT_TOOL_LOOP_EXTRA: usize = 50;
pub const REFLECTION_TOOL_LOOP_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy)]
pub enum ToolApprovalDecision {
    Approve,
    Deny,
}

/// Template variable values for system-prompt.md rendering
struct GhostContextVars {
    ghost_identity: String,
    ghost_diary: String,
    ghost_skills: String,
    system_info: String,
}

impl GhostContextVars {
    /// Convert to the slice format expected by the template engine
    fn as_pairs(&self) -> Vec<(&str, &str)> {
        vec![
            ("ghost_identity", self.ghost_identity.as_str()),
            ("ghost_diary", self.ghost_diary.as_str()),
            ("ghost_skills", self.ghost_skills.as_str()),
            ("system_info", self.system_info.as_str()),
        ]
    }
}

/// High-level chat interface that hides tools and conversation complexity
///
/// This is the SINGLE interface that Discord, WebSocket, and other transports
/// should use. It handles everything: history, system prompts, tool loops, etc.
pub struct SessionChat {
    pub(crate) tool_manager: ToolManager,
    knowledge_engine: Option<Arc<t_koma_knowledge::KnowledgeEngine>>,
    prompt_cache: PromptCacheManager,
    compaction_config: CompactionConfig,
    system_info: String,
    skill_paths: Vec<std::path::PathBuf>,
    dump_queries: bool,
}

async fn load_recent_active_diary_entries(
    diary_root: &std::path::Path,
    limit: usize,
) -> Vec<(NaiveDate, String)> {
    let mut entries = match tokio::fs::read_dir(diary_root).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut diary_files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
            continue;
        };
        diary_files.push((date, path));
    }

    diary_files.sort_by(|(a, _), (b, _)| b.cmp(a));

    let mut out = Vec::new();
    for (date, path) in diary_files {
        if out.len() >= limit {
            break;
        }

        if let Ok(content) = tokio::fs::read_to_string(path).await {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                out.push((date, trimmed.to_string()));
            }
        }
    }

    out
}

/// Discover available skills from all paths and build a prompt listing.
///
/// Ghost-local skills (from `workspace_root/skills/`) take highest priority,
/// then configured paths (user config, project defaults). Same-name skills
/// from lower-priority paths are overridden.
async fn discover_skills_listing(
    workspace_root: &std::path::Path,
    skill_paths: &[std::path::PathBuf],
) -> String {
    use std::collections::{HashMap, HashSet};
    let mut skills: HashMap<String, String> = HashMap::new();

    // Scan configured paths first (lowest priority) and track default skill names
    let mut default_names: HashSet<String> = HashSet::new();
    for dir in skill_paths.iter().rev() {
        if let Ok(found) = scan_skills_dir(dir).await {
            for (name, _) in &found {
                default_names.insert(name.clone());
            }
            skills.extend(found);
        }
    }

    // Ghost workspace skills: label only truly ghost-created ones (not synced defaults)
    let workspace_skills = workspace_root.join("skills");
    if let Ok(found) = scan_skills_dir(&workspace_skills).await {
        for (name, desc) in found {
            if default_names.contains(&name) {
                skills.insert(name, desc);
            } else {
                skills.insert(format!("{name} (ghost-created)"), desc);
            }
        }
    }

    if skills.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Available Skills".to_string(),
        String::new(),
        "Use `load_skill` to load the full instructions for any skill before using it.".to_string(),
        String::new(),
    ];
    let mut sorted: Vec<_> = skills.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, description) in sorted {
        lines.push(format!("- **{}**: {}", name, description));
    }
    lines.join("\n")
}

/// Scan a directory for skill subdirectories and extract name + description.
async fn scan_skills_dir(dir: &std::path::Path) -> std::io::Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    if !dir.exists() {
        return Ok(results);
    }

    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if let Ok(content) = tokio::fs::read_to_string(&skill_md).await
            && let Some((name, desc)) = parse_skill_frontmatter(&content)
        {
            results.push((name, desc));
        }
    }

    Ok(results)
}

/// Extract name and description from YAML frontmatter in a SKILL.md file.
fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return None;
    }
    let rest = &trimmed[3..];
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];

    let mut name = None;
    let mut description = None;
    for line in yaml.lines() {
        if let Some(val) = line.strip_prefix("name:") {
            name = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("description:") {
            description = Some(val.trim().to_string());
        }
    }

    Some((name?, description.unwrap_or_default()))
}

/// Sync skills from source paths into the ghost workspace skills directory.
///
/// For each skill found in the source paths (user config, project defaults),
/// copies the entire skill directory to `$WORKSPACE/skills/{name}/`.
/// Overwrites existing files if content has changed; skips if identical.
/// Ghost-created skills (those not present in source paths) are left untouched.
async fn sync_default_skills(workspace_root: &std::path::Path, skill_paths: &[std::path::PathBuf]) {
    use std::collections::HashSet;

    let dest_root = workspace_root.join("skills");

    // Collect all skill names from source paths (dedup: first occurrence wins)
    let mut seen: HashSet<String> = HashSet::new();

    for source_dir in skill_paths {
        let Ok(mut entries) = tokio::fs::read_dir(source_dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let src_path = entry.path();
            if !src_path.is_dir() {
                continue;
            }
            let Some(skill_name) = src_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // Only process each skill name once (highest priority source wins)
            if !seen.insert(skill_name.to_string()) {
                continue;
            }
            // Must have a SKILL.md to be considered a valid skill
            if !src_path.join("SKILL.md").exists() {
                continue;
            }
            let dest_skill = dest_root.join(skill_name);
            if let Err(e) = sync_skill_dir(&src_path, &dest_skill).await {
                tracing::warn!(skill = skill_name, error = %e, "Failed to sync skill");
            }
        }
    }
}

/// Recursively sync a single skill directory from source to destination.
///
/// Only writes files whose content differs from the destination, keeping
/// the operation idempotent and fast for repeated calls.
async fn sync_skill_dir(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dest).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dest.join(&file_name);

        if src_path.is_dir() {
            Box::pin(sync_skill_dir(&src_path, &dest_path)).await?;
        } else {
            let src_bytes = tokio::fs::read(&src_path).await?;
            let needs_write = match tokio::fs::read(&dest_path).await {
                Ok(dest_bytes) => dest_bytes != src_bytes,
                Err(_) => true,
            };
            if needs_write {
                tokio::fs::write(&dest_path, &src_bytes).await?;
                tracing::debug!(
                    file = %dest_path.display(),
                    "Synced skill file"
                );
            }
        }
    }

    Ok(())
}

/// Result of a detached job conversation.
pub struct JobChatResult {
    pub response_text: String,
    pub transcript: Vec<TranscriptEntry>,
}

/// Convert provider response blocks to DB content blocks.
fn provider_to_db_blocks(response: &ProviderResponse) -> Vec<DbContentBlock> {
    response
        .content
        .iter()
        .map(|block| match block {
            ProviderContentBlock::Text { text } => DbContentBlock::Text { text: text.clone() },
            ProviderContentBlock::ToolUse { id, name, input } => DbContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            ProviderContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => DbContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: *is_error,
            },
        })
        .collect()
}

impl SessionChat {
    /// Create a new SessionChat instance.
    ///
    /// `skill_paths` are directories to search for skills, in priority order
    /// (first match wins). Typically: user config skills, project defaults.
    /// Ghost workspace skills are prepended automatically at execution time.
    pub fn new(
        knowledge_engine: Option<Arc<t_koma_knowledge::KnowledgeEngine>>,
        skill_paths: Vec<std::path::PathBuf>,
        compaction_config: CompactionConfig,
    ) -> Self {
        Self {
            tool_manager: ToolManager::new_chat(skill_paths.clone()),
            knowledge_engine,
            prompt_cache: PromptCacheManager::new(),
            compaction_config,
            system_info: system_info::build_system_info(),
            skill_paths,
            dump_queries: false,
        }
    }

    /// Enable writing empty-response debug logs (gated by `dump_queries` config).
    pub fn with_dump_queries(mut self, enabled: bool) -> Self {
        self.dump_queries = enabled;
        self
    }

    /// Skill search paths (for constructing alternate ToolManagers).
    pub fn skill_paths(&self) -> &[std::path::PathBuf] {
        &self.skill_paths
    }

    /// Send an operator message and get the GHOST response
    ///
    /// This method handles the ENTIRE conversation flow:
    /// 1. Verifies session ownership
    /// 2. Saves the operator message to the database
    /// 3. Fetches conversation history
    /// 4. Builds system prompt with all available tools
    /// 5. Sends to the provider with full tool use loop support
    /// 6. Saves the ghost response to the database
    /// 7. Returns the final text response
    ///
    /// # Arguments
    /// * `pool` - The database pool
    /// * `ghost_id` - The ghost ID for this session
    /// * `provider` - The LLM provider
    /// * `provider_name` - The provider name for logging
    /// * `model` - The model name
    /// * `context_window_override` - Optional context window override
    /// * `session_id` - The session ID to chat in
    /// * `operator_id` - The operator ID (for session ownership verification)
    /// * `message` - The operator's message content
    /// * `tool_call_tx` - Optional sender for tool call summaries
    ///
    /// # Returns
    /// The final text response from the provider and tool call log
    #[allow(clippy::too_many_arguments)]
    pub async fn chat(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        message: &str,
        message_already_persisted: bool,
        tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
    ) -> Result<(String, Vec<ToolCallSummary>), ChatError> {
        // Verify session exists and belongs to operator
        let session = SessionRepository::get_by_id(pool.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        if session.operator_id != operator_id {
            return Err(ChatError::SessionNotFound);
        }

        info!(
            event_kind = "chat_io",
            "[session:{}] Chat message from operator {}", session_id, operator_id
        );

        // Save operator message to database (skip on retry — already persisted)
        if !message_already_persisted {
            let user_content = vec![DbContentBlock::Text {
                text: message.to_string(),
            }];
            SessionRepository::add_message(
                pool.pool(),
                ghost_id,
                session_id,
                MessageRole::Operator,
                user_content,
                None,
            )
            .await?;
        }

        // Build system prompt with ghost context (cached for 5 min)
        let system_blocks = self
            .build_cached_system_blocks(pool, ghost_id, session_id)
            .await?;

        // Load history (compaction-aware) and apply compaction if needed
        let api_messages = self
            .load_compacted_history(
                pool,
                provider,
                model,
                context_window_override,
                &session,
                &system_blocks,
            )
            .await?;

        // Send to provider with tool loop
        self.send_with_tool_loop(
            pool,
            ghost_id,
            provider,
            provider_name,
            model,
            context_window_override,
            session_id,
            operator_id,
            system_blocks,
            api_messages,
            None,
            DEFAULT_TOOL_LOOP_LIMIT,
            tool_call_tx,
        )
        .await
    }

    /// Run a background job conversation without persisting to session messages.
    ///
    /// Similar to `chat()` but collects the full transcript in memory instead
    /// of writing each message to the database. The caller is responsible for
    /// persisting the returned `JobChatResult` into the `job_logs` table.
    ///
    /// When `load_session_history` is true, the existing session messages are
    /// loaded as read-only context for the LLM (e.g. heartbeat needs
    /// conversation context).
    ///
    /// `tool_manager_override`: when `Some`, use this tool manager instead of
    /// `self.tool_manager` (e.g. reflection passes its own set).
    ///
    /// `job_handle`: when `Some`, inject into ToolContext so background-only
    /// tools like `reflection_todo` can persist state mid-run.
    #[allow(clippy::too_many_arguments)]
    pub async fn chat_job(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        prompt: &str,
        load_session_history: bool,
        tool_manager_override: Option<&ToolManager>,
        job_handle: Option<JobHandle>,
        max_tool_iterations: Option<usize>,
    ) -> Result<JobChatResult, ChatError> {
        // Verify session exists
        let session = SessionRepository::get_by_id(pool.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        if session.operator_id != operator_id {
            return Err(ChatError::SessionNotFound);
        }

        info!(
            event_kind = "chat_io",
            "[session:{}] Job chat (detached) for operator {}", session_id, operator_id
        );

        // Build system prompt (cached for 5 min)
        let system_blocks = self
            .build_cached_system_blocks(pool, ghost_id, session_id)
            .await?;

        // Optionally load session history (compaction-aware)
        let session_history = if load_session_history {
            self.load_compacted_history(
                pool,
                provider,
                model,
                context_window_override,
                &session,
                &system_blocks,
            )
            .await?
        } else {
            Vec::new()
        };

        // Initialize transcript with the prompt
        let mut transcript = vec![TranscriptEntry {
            role: MessageRole::Operator,
            content: vec![DbContentBlock::Text {
                text: prompt.to_string(),
            }],
            model: None,
        }];

        let tm = tool_manager_override.unwrap_or(&self.tool_manager);

        let limit = max_tool_iterations.unwrap_or(DEFAULT_TOOL_LOOP_LIMIT);
        let text = self
            .send_job_with_tool_loop(
                pool,
                ghost_id,
                provider,
                provider_name,
                model,
                session_id,
                operator_id,
                system_blocks,
                &session_history,
                &mut transcript,
                limit,
                tm,
                job_handle,
            )
            .await?;

        Ok(JobChatResult {
            response_text: text,
            transcript,
        })
    }

    /// Tool loop for detached job conversations.
    ///
    /// Instead of persisting each message to the DB, appends to the
    /// in-memory `transcript`. The LLM sees `session_history ++ transcript`
    /// on each iteration.
    #[allow(clippy::too_many_arguments)]
    async fn send_job_with_tool_loop(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        operator_id: &str,
        system_blocks: Vec<SystemBlock>,
        session_history: &[ChatMessage],
        transcript: &mut Vec<TranscriptEntry>,
        max_iterations: usize,
        tool_manager: &ToolManager,
        job_handle: Option<JobHandle>,
    ) -> Result<String, ChatError> {
        let tools = tool_manager.get_tools();

        // Build initial API messages: session history + transcript so far
        let mut api_messages: Vec<ChatMessage> = session_history.to_vec();
        api_messages.extend(build_transcript_messages(transcript));

        let mut response = send_with_retry(
            provider,
            Some(system_blocks.clone()),
            api_messages,
            tools.clone(),
        )
        .await
        .map_err(|e| {
            warn!("Job initial send failed after retries: {e:#}");
            ChatError::Provider(e)
        })?;
        Self::log_usage(pool, ghost_id, session_id, model, &response).await;

        let mut tool_context = self
            .load_tool_context(pool, ghost_id, operator_id, model)
            .await?;
        tool_context.job_handle = job_handle;

        for iteration in 0..max_iterations {
            if !has_tool_uses(&response) {
                break;
            }

            info!(
                "[session:{}] Job tool use (iteration {})",
                session_id,
                iteration + 1
            );

            // Append ghost response blocks to transcript
            transcript.push(TranscriptEntry {
                role: MessageRole::Ghost,
                content: provider_to_db_blocks(&response),
                model: Some(model.to_string()),
            });

            if iteration + 1 == max_iterations {
                return Err(ChatError::ToolLoopLimitReached(PendingToolContinuation {
                    pending_tool_uses: collect_pending_tool_uses(&response),
                    partial_transcript: transcript.clone(),
                }));
            }

            // Execute tools
            let tool_uses = collect_pending_tool_uses(&response);
            let mut tool_results = Vec::new();
            let mut _job_tool_log = Vec::new();
            self.execute_tool_uses_with(
                session_id,
                &tool_uses,
                pool,
                &mut tool_context,
                &mut tool_results,
                &mut _job_tool_log,
                tool_manager,
            )
            .await?;

            // Append tool results to transcript
            transcript.push(TranscriptEntry {
                role: MessageRole::Operator,
                content: tool_results,
                model: None,
            });

            // Rebuild API messages and re-send
            let mut api_messages: Vec<ChatMessage> = session_history.to_vec();
            api_messages.extend(build_transcript_messages(transcript));

            response = send_with_retry(
                provider,
                Some(system_blocks.clone()),
                api_messages,
                tools.clone(),
            )
            .await
            .map_err(|e| {
                warn!("Job tool-loop send failed after retries (iteration {iteration}): {e:#}");
                ChatError::Provider(e)
            })?;
            Self::log_usage(pool, ghost_id, session_id, model, &response).await;
        }

        // Extract final text and append to transcript
        let text = extract_all_text(&response);
        let text = text.trim().to_string();
        if text.is_empty() {
            // raw_json is only populated when dump_queries is enabled.
            if let Some(raw_json) = &response.raw_json {
                let log_path = self.write_empty_response_log(session_id, raw_json).await;
                warn!(
                    event_kind = "empty_response_debug",
                    session_id = session_id,
                    provider = provider_name,
                    model = model,
                    log_path = ?log_path,
                    "Empty response detected in job. Full raw response written to log file."
                );
            }
            return Err(ChatError::EmptyResponse);
        }

        info!(
            event_kind = "chat_io",
            "[session:{}] Job final response ({} / {}): {}",
            session_id,
            provider_name,
            model,
            if text.len() > 100 {
                &text[..text.floor_char_boundary(100)]
            } else {
                &text
            }
        );

        transcript.push(TranscriptEntry {
            role: MessageRole::Ghost,
            content: vec![DbContentBlock::Text { text: text.clone() }],
            model: Some(model.to_string()),
        });

        Ok(text)
    }

    /// Internal method: Send conversation to the provider with full tool use loop.
    ///
    /// Returns the final text response and a log of all tool calls executed.
    ///
    /// If `tool_call_tx` is provided, tool call summaries are sent incrementally
    /// after each iteration instead of only being returned at the end.
    #[allow(clippy::too_many_arguments)]
    async fn send_with_tool_loop(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        system_blocks: Vec<SystemBlock>,
        api_messages: Vec<ChatMessage>,
        new_message: Option<&str>,
        max_iterations: usize,
        tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
    ) -> Result<(String, Vec<ToolCallSummary>), ChatError> {
        let tools = self.tool_manager.get_tools();
        let mut tool_call_log: Vec<ToolCallSummary> = Vec::new();
        let mut prev_tool_count: usize = 0;

        // Initial request to the provider
        let mut response = provider
            .send_conversation(
                Some(system_blocks.clone()),
                api_messages.clone(),
                tools.clone(),
                new_message,
                None,
                None,
            )
            .await
            .map_err(ChatError::Provider)?;
        Self::log_usage(pool, ghost_id, session_id, model, &response).await;

        // Handle tool use loop (bounded to prevent infinite loops)
        let mut tool_context = self
            .load_tool_context(pool, ghost_id, operator_id, model)
            .await?;
        for iteration in 0..max_iterations {
            let has_tool_use = has_tool_uses(&response);

            if !has_tool_use {
                break;
            }

            info!(
                "[session:{}] Tool use detected (iteration {})",
                session_id,
                iteration + 1
            );

            // Save ghost message with tool_use blocks
            self.save_ghost_response(pool, session_id, model, &response)
                .await?;

            if iteration + 1 == max_iterations {
                return Err(ChatError::ToolLoopLimitReached(PendingToolContinuation {
                    pending_tool_uses: collect_pending_tool_uses(&response),
                    // Messages already persisted to DB — no in-memory transcript.
                    partial_transcript: Vec::new(),
                }));
            }

            // Execute tools and get results
            let tool_results = match self
                .execute_tools_from_response(
                    session_id,
                    &response,
                    pool,
                    &mut tool_context,
                    &mut tool_call_log,
                )
                .await
            {
                Ok(results) => results,
                Err(ChatError::ToolLoopLimitReached(pending)) => {
                    return Err(ChatError::ToolLoopLimitReached(pending));
                }
                Err(e) => return Err(e),
            };

            // Stream new tool calls to the transport layer if a sender is provided
            if let Some(tx) = &tool_call_tx {
                let new_calls = tool_call_log[prev_tool_count..].to_vec();
                if !new_calls.is_empty() {
                    let _ = tx.send(new_calls);
                }
                prev_tool_count = tool_call_log.len();
            }

            // Save tool results to database
            self.save_tool_results(pool, ghost_id, session_id, &tool_results)
                .await?;

            // Rebuild history with masking only (no Phase 2 mid-tool-loop)
            let history = SessionRepository::get_messages(pool.pool(), session_id).await?;
            let raw_messages = build_history_messages(&history, None);
            let tool_refs: Vec<&dyn crate::tools::Tool> = tools.to_vec();
            let new_api_messages = self.apply_masking_if_needed(
                model,
                context_window_override,
                &system_blocks,
                &tool_refs,
                raw_messages,
            );

            // Send tool results back to the provider
            response = provider
                .send_conversation(
                    Some(system_blocks.clone()),
                    new_api_messages,
                    tools.clone(),
                    None,
                    None,
                    None,
                )
                .await
                .map_err(ChatError::Provider)?;
            Self::log_usage(pool, ghost_id, session_id, model, &response).await;
        }

        // Extract and save final text response
        let text = self
            .finalize_response(pool, session_id, provider_name, model, &response)
            .await?;

        Ok((text, tool_call_log))
    }

    /// Save a ghost response (with tool_use blocks) to the database
    async fn save_ghost_response(
        &self,
        pool: &KomaDbPool,
        session_id: &str,
        model: &str,
        response: &ProviderResponse,
    ) -> Result<(), ChatError> {
        let ghost_content = provider_to_db_blocks(response);

        // Get ghost_id from session
        let session = SessionRepository::get_by_id(pool.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        SessionRepository::add_message(
            pool.pool(),
            &session.ghost_id,
            session_id,
            MessageRole::Ghost,
            ghost_content,
            Some(model),
        )
        .await?;
        Ok(())
    }

    /// Execute all tool_use blocks from a response and return the results.
    async fn execute_tools_from_response(
        &self,
        session_id: &str,
        response: &ProviderResponse,
        pool: &KomaDbPool,
        tool_context: &mut ToolContext,
        tool_call_log: &mut Vec<ToolCallSummary>,
    ) -> Result<Vec<DbContentBlock>, ChatError> {
        let tool_uses = collect_pending_tool_uses(response);

        let mut tool_results = Vec::new();
        self.execute_tool_uses(
            session_id,
            &tool_uses,
            pool,
            tool_context,
            &mut tool_results,
            tool_call_log,
        )
        .await?;

        Ok(tool_results)
    }

    async fn execute_tool_uses(
        &self,
        session_id: &str,
        tool_uses: &[PendingToolUse],
        pool: &KomaDbPool,
        tool_context: &mut ToolContext,
        tool_results: &mut Vec<DbContentBlock>,
        tool_call_log: &mut Vec<ToolCallSummary>,
    ) -> Result<(), ChatError> {
        self.execute_tool_uses_with(
            session_id,
            tool_uses,
            pool,
            tool_context,
            tool_results,
            tool_call_log,
            &self.tool_manager,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_uses_with(
        &self,
        session_id: &str,
        tool_uses: &[PendingToolUse],
        pool: &KomaDbPool,
        tool_context: &mut ToolContext,
        tool_results: &mut Vec<DbContentBlock>,
        tool_call_log: &mut Vec<ToolCallSummary>,
        tool_manager: &ToolManager,
    ) -> Result<(), ChatError> {
        for (index, tool_use) in tool_uses.iter().enumerate() {
            info!(
                "[session:{}] Executing tool: {} (id: {})",
                session_id, tool_use.name, tool_use.id
            );

            let input_preview = build_input_preview(&tool_use.input);

            let result = tool_manager
                .execute_with_context(&tool_use.name, tool_use.input.clone(), tool_context)
                .await;

            let (content, is_error) = match result {
                Ok(output) => (output, false),
                Err(e) => {
                    if let Some(reason) = ApprovalReason::parse(&e) {
                        return Err(ChatError::ToolApprovalRequired(PendingToolApproval {
                            pending_tool_uses: tool_uses[index..].to_vec(),
                            completed_results: tool_results.clone(),
                            reason,
                        }));
                    }
                    (format!("Error: {}", e), true)
                }
            };

            tool_call_log.push(ToolCallSummary {
                name: tool_use.name.clone(),
                input_preview,
                output_preview: truncate_preview(&content, 100),
                is_error,
            });

            tool_results.push(DbContentBlock::ToolResult {
                tool_use_id: tool_use.id.clone(),
                content,
                is_error: None,
            });

            self.persist_tool_context(pool, tool_context).await?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn resume_tool_approval(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        pending: PendingToolApproval,
        decision: ToolApprovalDecision,
    ) -> Result<String, ChatError> {
        let mut tool_context = self
            .load_tool_context(pool, ghost_id, operator_id, model)
            .await?;

        let mut tool_results = pending.completed_results;
        match decision {
            ToolApprovalDecision::Approve => {
                tool_context.apply_approval(&pending.reason);
                self.persist_tool_context(pool, &mut tool_context).await?;
                let mut _approval_tool_log = Vec::new();
                self.execute_tool_uses(
                    session_id,
                    &pending.pending_tool_uses,
                    pool,
                    &mut tool_context,
                    &mut tool_results,
                    &mut _approval_tool_log,
                )
                .await?;
            }
            ToolApprovalDecision::Deny => {
                let denial = pending.reason.denial_message();
                for (index, tool_use) in pending.pending_tool_uses.iter().enumerate() {
                    let content = if index == 0 {
                        denial.to_string()
                    } else {
                        "Error: Skipped because approval was denied.".to_string()
                    };
                    tool_results.push(DbContentBlock::ToolResult {
                        tool_use_id: tool_use.id.clone(),
                        content,
                        is_error: None,
                    });
                }
            }
        }

        self.save_tool_results(pool, ghost_id, session_id, &tool_results)
            .await?;

        self.resume_after_tool_results(
            pool,
            ghost_id,
            provider,
            provider_name,
            model,
            context_window_override,
            session_id,
            operator_id,
            DEFAULT_TOOL_LOOP_LIMIT,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn resume_tool_loop(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        pending: PendingToolContinuation,
        extra_iterations: usize,
    ) -> Result<String, ChatError> {
        let mut tool_context = self
            .load_tool_context(pool, ghost_id, operator_id, model)
            .await?;
        let mut tool_results = Vec::new();
        let mut _resume_tool_log = Vec::new();
        self.execute_tool_uses(
            session_id,
            &pending.pending_tool_uses,
            pool,
            &mut tool_context,
            &mut tool_results,
            &mut _resume_tool_log,
        )
        .await?;

        self.save_tool_results(pool, ghost_id, session_id, &tool_results)
            .await?;

        self.resume_after_tool_results(
            pool,
            ghost_id,
            provider,
            provider_name,
            model,
            context_window_override,
            session_id,
            operator_id,
            extra_iterations,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn resume_after_tool_results(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        context_window_override: Option<u32>,
        session_id: &str,
        operator_id: &str,
        max_iterations: usize,
    ) -> Result<String, ChatError> {
        let session = SessionRepository::get_by_id(pool.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        let system_blocks = self
            .build_cached_system_blocks(pool, ghost_id, session_id)
            .await?;

        let api_messages = self
            .load_compacted_history(
                pool,
                provider,
                model,
                context_window_override,
                &session,
                &system_blocks,
            )
            .await?;

        let (text, _tool_calls) = self
            .send_with_tool_loop(
                pool,
                ghost_id,
                provider,
                provider_name,
                model,
                context_window_override,
                session_id,
                operator_id,
                system_blocks,
                api_messages,
                None,
                max_iterations,
                None,
            )
            .await?;
        Ok(text)
    }

    async fn load_tool_context(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        operator_id: &str,
        model: &str,
    ) -> Result<ToolContext, ChatError> {
        // Fetch ghost to get name and derive workspace path
        let ghost = GhostRepository::get_by_id(pool.pool(), ghost_id)
            .await?
            .ok_or_else(|| t_koma_db::DbError::GhostNotFound(ghost_id.to_string()))?;

        let ghost_name = ghost.name;
        let tool_state = GhostRepository::get_tool_state_by_name(pool.pool(), &ghost_name).await?;

        let workspace_root = ghost_workspace_path(&ghost_name)?;
        let mut cwd = tool_state
            .cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| workspace_root.clone());

        if !cwd.is_absolute() {
            cwd = workspace_root.join(cwd);
        }

        let mut context = ToolContext::new(ghost_name, workspace_root.clone(), cwd, false);
        context.set_model_id(model.to_string());
        let operator = OperatorRepository::get_by_id(pool.pool(), operator_id)
            .await?
            .ok_or_else(|| t_koma_db::DbError::OperatorNotFound(operator_id.to_string()))?;
        context.set_operator_access_level(operator.access_level);
        let allow_escape = operator.access_level == t_koma_db::OperatorAccessLevel::PuppetMaster
            || operator.allow_workspace_escape;
        context.set_allow_workspace_escape(allow_escape);
        if let Some(engine) = &self.knowledge_engine {
            context = context.with_knowledge_engine(Arc::clone(engine));
        }

        if !is_within_workspace(&context, context.cwd()) {
            context.set_cwd(workspace_root.clone());
        }

        let cwd_missing = tokio::fs::metadata(context.cwd()).await.is_err();
        if cwd_missing {
            context.set_cwd(workspace_root);
        }

        if context.is_dirty() {
            self.persist_tool_context(pool, &mut context).await?;
        }

        Ok(context)
    }

    async fn persist_tool_context(
        &self,
        pool: &KomaDbPool,
        context: &mut ToolContext,
    ) -> Result<(), ChatError> {
        if !context.is_dirty() {
            return Ok(());
        }

        let cwd = context.cwd().to_string_lossy().to_string();
        GhostRepository::update_tool_state_by_name(pool.pool(), context.ghost_name(), &cwd).await?;
        context.clear_dirty();
        Ok(())
    }

    /// Load history messages with compaction awareness.
    ///
    /// If the session already has a compaction summary from a previous run,
    /// loads only messages after the cursor and prepends the summary as a
    /// synthetic user message. Then runs `compact_if_needed()` to handle
    /// any further growth since the last compaction.
    ///
    /// Persists new compaction state to the DB if Phase 2 ran.
    #[allow(clippy::too_many_arguments)]
    async fn load_compacted_history(
        &self,
        pool: &KomaDbPool,
        provider: &dyn Provider,
        model: &str,
        context_window_override: Option<u32>,
        session: &Session,
        system_blocks: &[SystemBlock],
    ) -> Result<Vec<ChatMessage>, ChatError> {
        // Load messages: if we have a compaction cursor, load only newer messages
        let raw_messages = if let Some(cursor_id) = &session.compaction_cursor_id {
            SessionRepository::get_messages_after(pool.pool(), &session.id, cursor_id).await?
        } else {
            SessionRepository::get_messages(pool.pool(), &session.id).await?
        };

        let mut api_messages = build_history_messages(&raw_messages, None);

        // Prepend existing compaction summary as a synthetic user message
        if let Some(summary) = &session.compaction_summary {
            let summary_msg = ChatMessage {
                role: ChatRole::User,
                content: vec![ChatContentBlock::Text {
                    text: format!(
                        "[Conversation summary — earlier messages compacted]\n\n{summary}"
                    ),
                    cache_control: None,
                }],
            };
            api_messages.insert(0, summary_msg);
        }

        // Run compaction if context budget is exceeded
        let tools = self.tool_manager.get_tools();
        let tool_refs: Vec<&dyn crate::tools::Tool> = tools.to_vec();

        if let Some(result) = compact_if_needed(
            model,
            context_window_override,
            system_blocks,
            &tool_refs,
            &api_messages,
            &self.compaction_config,
            provider,
        )
        .await
        {
            // If Phase 2 (LLM summarization) produced a summary, persist it.
            // The cursor must point to the last raw message that was *summarized*,
            // not the last message overall. Messages after the cursor are kept
            // verbatim and loaded on the next request.
            if let Some(ref summary) = result.summary {
                let has_synthetic_prefix = session.compaction_summary.is_some();
                let raw_summarized = if has_synthetic_prefix {
                    // The synthetic summary prefix occupies one slot in compacted_count
                    result.compacted_count.saturating_sub(1)
                } else {
                    result.compacted_count
                };

                if raw_summarized == 0 || raw_summarized > raw_messages.len() {
                    warn!(
                        session_id = session.id,
                        compacted_count = result.compacted_count,
                        raw_len = raw_messages.len(),
                        has_synthetic_prefix,
                        "Unexpected compaction count — skipping cursor update"
                    );
                } else if let Err(e) = SessionRepository::update_compaction(
                    pool.pool(),
                    &session.id,
                    summary,
                    &raw_messages[raw_summarized - 1].id,
                )
                .await
                {
                    warn!(
                        session_id = session.id,
                        error = %e,
                        "Failed to persist compaction state"
                    );
                } else {
                    info!(
                        session_id = session.id,
                        compacted_count = result.compacted_count,
                        masked = result.masked,
                        summarized = result.summarized,
                        "Compaction state persisted"
                    );
                }
            }

            Ok(result.messages)
        } else {
            Ok(api_messages)
        }
    }

    /// Apply Phase 1 masking only (no LLM calls) during tool loop iterations.
    ///
    /// This is a lightweight version of compaction used mid-tool-loop to keep
    /// context usage reasonable without the overhead of an LLM summarization call.
    fn apply_masking_if_needed(
        &self,
        model: &str,
        context_window_override: Option<u32>,
        system_blocks: &[SystemBlock],
        tools: &[&dyn crate::tools::Tool],
        messages: Vec<ChatMessage>,
    ) -> Vec<ChatMessage> {
        use crate::chat::token_budget::compute_budget;

        let budget = compute_budget(
            model,
            context_window_override,
            system_blocks,
            tools,
            &messages,
            self.compaction_config.threshold,
        );

        if budget.needs_compaction {
            mask_tool_results(&messages, &self.compaction_config)
        } else {
            messages
        }
    }

    /// Build system prompt blocks with caching.
    ///
    /// Returns cached blocks if the ghost context hasn't changed within the
    /// 5-minute TTL, otherwise rebuilds and caches fresh blocks.
    async fn build_cached_system_blocks(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        session_id: &str,
    ) -> Result<Vec<SystemBlock>, ChatError> {
        // Fetch ghost to get name and derive workspace path
        let ghost = GhostRepository::get_by_id(pool.pool(), ghost_id)
            .await?
            .ok_or_else(|| t_koma_db::DbError::GhostNotFound(ghost_id.to_string()))?;

        let workspace_root = ghost_workspace_path(&ghost.name)?;

        // Build context vars to compute hash
        let ghost_vars = self.build_ghost_context_vars(&workspace_root).await?;
        let pairs = ghost_vars.as_pairs();
        let ctx_hash = hash_context(&pairs);

        // Use cache: only rebuilds if hash changed or TTL expired
        let blocks = self
            .prompt_cache
            .get_or_build(session_id, pool.pool(), ghost_id, &ctx_hash, || {
                let system_prompt = SystemPrompt::new(&pairs);
                async move { build_system_prompt(&system_prompt) }
            })
            .await;

        Ok(blocks)
    }

    /// Build template variables for system-prompt.md rendering
    ///
    /// Collects identity files, diary entries, and skill listings from the
    /// ghost workspace into string values for template substitution.
    async fn build_ghost_context_vars(
        &self,
        workspace_root: &std::path::Path,
    ) -> Result<GhostContextVars, ChatError> {
        // Ghost identity (BOOT.md + SOUL.md + USER.md)
        let mut identity_parts = Vec::new();
        for (label, filename) in [
            ("BOOT.md", "BOOT.md"),
            ("SOUL.md", "SOUL.md"),
            ("USER.md", "USER.md"),
        ] {
            let path = workspace_root.join(filename);
            if let Ok(content) = tokio::fs::read_to_string(&path).await
                && !content.trim().is_empty()
            {
                identity_parts.push(format!("# {}\n\n{}", label, content.trim()));
            }
        }
        let ghost_identity = identity_parts.join("\n\n");

        // Diary entries (two most recent active days)
        let diary_root = workspace_root.join("diary");
        let mut diary_parts = Vec::new();
        let diary_entries = load_recent_active_diary_entries(&diary_root, 2).await;
        for (day, content) in diary_entries {
            diary_parts.push(format!("# Diary {}\n\n{}", day.format("%Y-%m-%d"), content));
        }
        let ghost_diary = diary_parts.join("\n\n");

        // Sync default skills into ghost workspace so they're accessible via read_file
        sync_default_skills(workspace_root, &self.skill_paths).await;

        // Available skills (ghost-local override config/project)
        let ghost_skills = discover_skills_listing(workspace_root, &self.skill_paths).await;

        Ok(GhostContextVars {
            ghost_identity,
            ghost_diary,
            ghost_skills,
            system_info: self.system_info.clone(),
        })
    }

    /// Save tool results to the database
    async fn save_tool_results(
        &self,
        pool: &KomaDbPool,
        ghost_id: &str,
        session_id: &str,
        tool_results: &[DbContentBlock],
    ) -> Result<(), ChatError> {
        SessionRepository::add_message(
            pool.pool(),
            ghost_id,
            session_id,
            MessageRole::Operator,
            tool_results.to_vec(),
            None,
        )
        .await?;
        Ok(())
    }

    /// Extract final text response and save it to the database
    async fn finalize_response(
        &self,
        pool: &KomaDbPool,
        session_id: &str,
        provider_name: &str,
        model: &str,
        response: &ProviderResponse,
    ) -> Result<String, ChatError> {
        let text = extract_all_text(response);
        let text = text.trim().to_string();
        if text.is_empty() {
            // raw_json is only populated when dump_queries is enabled.
            if let Some(raw_json) = &response.raw_json {
                let log_path = self.write_empty_response_log(session_id, raw_json).await;
                warn!(
                    event_kind = "empty_response_debug",
                    session_id = session_id,
                    provider = provider_name,
                    model = model,
                    log_path = ?log_path,
                    "Empty response detected. Full raw response written to log file."
                );
            }
            return Err(ChatError::EmptyResponse);
        }

        info!(
            event_kind = "chat_io",
            "[session:{}] GHOST final response ({} / {}): {}",
            session_id,
            provider_name,
            model,
            if text.len() > 100 {
                &text[..text.floor_char_boundary(100)]
            } else {
                &text
            }
        );

        // Get ghost_id from session
        let session = SessionRepository::get_by_id(pool.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        let final_content = vec![DbContentBlock::Text { text: text.clone() }];
        SessionRepository::add_message(
            pool.pool(),
            &session.ghost_id,
            session_id,
            MessageRole::Ghost,
            final_content,
            Some(model),
        )
        .await?;

        Ok(text)
    }

    /// Write raw JSON response to a debug log file when empty response is detected.
    /// Returns the path to the written file.
    /// Only called when `dump_queries` is enabled (raw_json is `None` otherwise).
    async fn write_empty_response_log(&self, session_id: &str, raw_json: &str) -> PathBuf {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("empty_response_{}_{}.json", session_id, timestamp);
        let log_path = std::env::temp_dir()
            .join("t-koma")
            .join("empty_response_logs")
            .join(&filename);

        // Ensure directory exists
        if let Some(parent) = log_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        // Write the file (best effort)
        let _ = tokio::fs::write(&log_path, raw_json).await;

        log_path
    }

    /// Log API usage data (fire-and-forget; failures are warned, not propagated).
    async fn log_usage(
        pool: &KomaDbPool,
        ghost_id: &str,
        session_id: &str,
        model: &str,
        response: &ProviderResponse,
    ) {
        let Some(usage) = &response.usage else {
            return;
        };
        let log = UsageLog::new(
            ghost_id,
            session_id,
            None,
            model,
            TokenUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: usage.cache_read_tokens.unwrap_or(0),
                cache_creation_tokens: usage.cache_creation_tokens.unwrap_or(0),
            },
        );
        if let Err(e) = UsageLogRepository::insert(pool.pool(), &log).await {
            warn!(session_id, error = %e, "Failed to log API usage");
        }
    }
}

impl Default for SessionChat {
    fn default() -> Self {
        Self::new(None, vec![], CompactionConfig::default())
    }
}

/// Build a compact key=value preview of tool input JSON (~80 chars max).
fn build_input_preview(input: &Value) -> String {
    let Some(obj) = input.as_object() else {
        let s = input.to_string();
        return truncate_preview(&s, 80);
    };

    let mut parts = Vec::new();
    for (key, val) in obj {
        let v = match val {
            Value::String(s) => truncate_preview(s, 30),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => {
                let s = val.to_string();
                truncate_preview(&s, 30)
            }
        };
        parts.push(format!("{key}={v}"));
    }

    truncate_preview(&parts.join(", "), 80)
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a safe char boundary at or before the target byte offset
        let target = max.saturating_sub(1);
        let boundary = s.floor_char_boundary(target);
        format!("{}…", &s[..boundary])
    }
}

fn collect_pending_tool_uses(response: &ProviderResponse) -> Vec<PendingToolUse> {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ProviderContentBlock::ToolUse { id, name, input } => Some(PendingToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Retry a provider `send_conversation` call with exponential backoff.
///
/// Used only in background job loops (reflection, heartbeat) where transient
/// failures (overloaded, 5xx, 429) should not immediately abort the job.
/// Interactive chat does NOT retry — fast feedback is more important there.
const JOB_RETRY_ATTEMPTS: u32 = 3;
const JOB_RETRY_BASE_SECS: u64 = 2;

async fn send_with_retry(
    provider: &dyn Provider,
    system: Option<Vec<SystemBlock>>,
    history: Vec<ChatMessage>,
    tools: Vec<&dyn crate::tools::Tool>,
) -> Result<ProviderResponse, ProviderError> {
    let mut last_err = None;
    for attempt in 0..JOB_RETRY_ATTEMPTS {
        match provider
            .send_conversation(
                system.clone(),
                history.clone(),
                tools.clone(),
                None,
                None,
                None,
            )
            .await
        {
            Ok(response) => return Ok(response),
            Err(e) if e.is_retryable() && attempt + 1 < JOB_RETRY_ATTEMPTS => {
                let delay = JOB_RETRY_BASE_SECS * 2u64.pow(attempt);
                warn!(
                    "Retryable provider error (attempt {}/{}), retrying in {delay}s: {e:#}",
                    attempt + 1,
                    JOB_RETRY_ATTEMPTS
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    let err = last_err.unwrap();
    warn!("Provider error persisted after {JOB_RETRY_ATTEMPTS} attempts, giving up: {err:#}");
    Err(err)
}
