use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;
#[cfg(feature = "live-tests")]
use tracing::info;
use tracing::{error, warn};

use crate::chat::compaction::CompactionConfig;
use crate::content::ids;
use crate::gateway_message;
use crate::providers::provider::Provider;
#[cfg(feature = "live-tests")]
use crate::providers::provider::{ProviderResponse, extract_all_text};
use crate::scheduler::{JobKind, SchedulerState};
use crate::session::{
    ChatError, DEFAULT_TOOL_LOOP_EXTRA, PendingToolApproval, PendingToolContinuation, SessionChat,
    ToolApprovalDecision,
};
#[cfg(feature = "live-tests")]
use crate::tools::ToolContext;
use t_koma_db::DbError;

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PendingInterface {
    platform: t_koma_db::Platform,
    external_id: String,
}

#[derive(Debug, Clone)]
pub struct PendingGatewayAction {
    pub operator_id: String,
    pub ghost_name: String,
    pub session_id: String,
    pub external_id: String,
    pub channel_id: String,
    pub intent: String,
    pub payload: Option<String>,
    pub expires_at: i64,
}

#[derive(Debug, Default)]
struct OperatorRateLimitState {
    last_5m: VecDeque<i64>,
    last_1h: VecDeque<i64>,
}

#[derive(Debug, Clone, Copy)]
pub struct HeartbeatOverride {
    pub next_due: i64,
    pub last_seen_updated_at: i64,
}

#[derive(Debug)]
pub enum RateLimitDecision {
    Allowed,
    Limited { retry_after: Duration },
}

/// Summary of a single tool call for operator visibility.
#[derive(Debug, Clone)]
pub struct ToolCallSummary {
    pub name: String,
    pub input_preview: String,
    pub output_preview: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub struct ChatResult {
    pub text: String,
    pub compaction_happened: bool,
    pub tool_calls: Vec<ToolCallSummary>,
}

/// Log entry for broadcasting events to listeners
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogEntry {
    /// Discord message received
    DiscordMessage {
        channel: String,
        user: String,
        content: String,
    },
    /// AI response sent to Discord
    DiscordResponse { user: String, content: String },
    /// HTTP request handled
    HttpRequest {
        method: String,
        path: String,
        status: u16,
    },
    /// WebSocket event
    WebSocket { event: String, client_id: String },
    /// General info message
    Info { message: String },
    /// Operator message received via chat
    OperatorMessage {
        operator_id: String,
        ghost_name: String,
        content: String,
    },
    /// Ghost response sent back to operator
    GhostMessage { ghost_name: String, content: String },
    /// Heartbeat runner status
    Heartbeat {
        ghost_name: String,
        session_id: String,
        status: String,
    },
    /// Reflection job status
    Reflection {
        ghost_name: String,
        session_id: String,
        status: String,
    },
    /// Routing decision for operator -> ghost/session
    Routing {
        platform: String,
        operator_id: String,
        ghost_name: String,
        session_id: String,
    },
    /// Generic tracing event from gateway runtime
    Trace {
        level: String,
        target: String,
        message: String,
    },
}

impl std::fmt::Display for LogEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use chrono::Utc;
        let timestamp = Utc::now().format("%H:%M:%S");

        match self {
            LogEntry::DiscordMessage {
                channel,
                user,
                content,
            } => write!(
                f,
                "[{}] [DISCORD] #{} @{}: {}",
                timestamp, channel, user, content
            ),
            LogEntry::DiscordResponse { user, content } => write!(
                f,
                "[{}] [AI] -> @{}: {}",
                timestamp,
                user,
                if content.len() > 50 {
                    format!("{}...", &content[..content.floor_char_boundary(50)])
                } else {
                    content.clone()
                }
            ),
            LogEntry::HttpRequest {
                method,
                path,
                status,
            } => write!(f, "[{}] [HTTP] {} {} {}", timestamp, method, path, status),
            LogEntry::WebSocket { event, client_id } => {
                write!(f, "[{}] [WS] {} {}", timestamp, event, client_id)
            }
            LogEntry::Info { message } => {
                write!(f, "[{}] [INFO] {}", timestamp, message)
            }
            LogEntry::OperatorMessage {
                operator_id,
                ghost_name,
                content,
            } => write!(
                f,
                "[{}] [OP] {} -> {}: {}",
                timestamp, operator_id, ghost_name, content
            ),
            LogEntry::GhostMessage {
                ghost_name,
                content,
            } => write!(
                f,
                "[{}] [GHOST] {} -> operator: {}",
                timestamp, ghost_name, content
            ),
            LogEntry::Heartbeat {
                ghost_name,
                session_id,
                status,
            } => write!(
                f,
                "[{}] [HEARTBEAT] {} ({}) {}",
                timestamp, ghost_name, session_id, status
            ),
            LogEntry::Reflection {
                ghost_name,
                session_id,
                status,
            } => write!(
                f,
                "[{}] [REFLECTION] {} ({}) {}",
                timestamp, ghost_name, session_id, status
            ),
            LogEntry::Routing {
                platform,
                operator_id,
                ghost_name,
                session_id,
            } => write!(
                f,
                "[{}] [ROUTE] {} {} -> {} ({})",
                timestamp, platform, operator_id, ghost_name, session_id
            ),
            LogEntry::Trace {
                level,
                target,
                message,
            } => write!(f, "[{}] [{}] {} {}", timestamp, level, target, message),
        }
    }
}

static GLOBAL_LOG_TX: OnceLock<broadcast::Sender<LogEntry>> = OnceLock::new();

pub fn emit_global_log(entry: LogEntry) {
    if let Some(tx) = GLOBAL_LOG_TX.get() {
        let _ = tx.send(entry);
    }
}

fn render_message(id: &str, vars: &[(&str, &str)]) -> String {
    gateway_message::from_content(id, None, vars).text_fallback
}

/// Shared application state
///
/// This holds all shared resources and provides the main interface for
/// handling chat conversations through `session_chat`.
pub struct AppState {
    /// Default model alias
    default_model_alias: String,
    /// Model registry keyed by alias
    models: HashMap<String, ModelEntry>,
    /// Log broadcast channel
    log_tx: broadcast::Sender<LogEntry>,
    /// T-KOMA database pool
    pub koma_db: t_koma_db::KomaDbPool,
    /// Active ghost name per operator
    active_ghosts: RwLock<HashMap<String, String>>,
    /// Pending interface selections (platform + external_id)
    pending_interfaces: RwLock<HashMap<String, PendingInterface>>,
    /// Pending tool approvals keyed by operator/ghost/session
    pending_tool_approvals: RwLock<HashMap<String, PendingToolApproval>>,
    /// Pending tool loop continuations keyed by operator/ghost/session
    pending_tool_loops: RwLock<HashMap<String, PendingToolContinuation>>,
    /// Pending Discord gateway actions keyed by opaque token
    pending_gateway_actions: RwLock<HashMap<String, PendingGatewayAction>>,
    /// Active chat requests keyed by operator/ghost/session
    in_flight_chats: RwLock<HashSet<String>>,
    /// Last ignored message keyed by operator/ghost/session
    ignored_messages: RwLock<HashMap<String, String>>,
    /// Per-operator message rate limit windows
    operator_rate_limits: RwLock<HashMap<String, OperatorRateLimitState>>,
    /// High-level chat interface - handles all conversation logic including tools
    pub session_chat: SessionChat,

    /// Persistent knowledge engine (DB pool opened once at startup)
    knowledge_engine: Arc<t_koma_knowledge::KnowledgeEngine>,

    /// Shared knowledge watcher handle
    shared_knowledge_watcher: RwLock<Option<JoinHandle<()>>>,

    /// Ghost knowledge watcher handles by ghost name
    ghost_knowledge_watchers: RwLock<HashMap<String, JoinHandle<()>>>,

    /// Heartbeat runner handle
    heartbeat_runner: RwLock<Option<JoinHandle<()>>>,

    /// Heartbeat override schedule per session (chat_key)
    heartbeat_overrides: RwLock<HashMap<String, HeartbeatOverride>>,

    /// Scheduler state for background jobs (heartbeat now, cron later)
    scheduler: RwLock<SchedulerState>,
    /// Discord bot token (optional, used by server-side Discord notifications)
    discord_bot_token: RwLock<Option<String>>,
}

/// Model entry tracked by the gateway
pub struct ModelEntry {
    pub alias: String,
    pub provider: String,
    pub model: String,
    pub client: Arc<dyn Provider>,
    /// Optional override for context window size (from config).
    pub context_window: Option<u32>,
}

impl AppState {
    /// Create a new AppState with the given model registry and database
    pub fn new(
        default_model_alias: String,
        models: HashMap<String, ModelEntry>,
        koma_db: t_koma_db::KomaDbPool,
        knowledge_engine: Arc<t_koma_knowledge::KnowledgeEngine>,
        skill_paths: Vec<std::path::PathBuf>,
        compaction_config: CompactionConfig,
    ) -> Self {
        let (log_tx, _) = broadcast::channel(100);
        let _ = GLOBAL_LOG_TX.set(log_tx.clone());
        let session_chat = SessionChat::new(
            Some(Arc::clone(&knowledge_engine)),
            skill_paths,
            compaction_config,
        );

        Self {
            default_model_alias,
            models,
            log_tx,
            koma_db,
            active_ghosts: RwLock::new(HashMap::new()),
            pending_interfaces: RwLock::new(HashMap::new()),
            pending_tool_approvals: RwLock::new(HashMap::new()),
            pending_tool_loops: RwLock::new(HashMap::new()),
            pending_gateway_actions: RwLock::new(HashMap::new()),
            in_flight_chats: RwLock::new(HashSet::new()),
            ignored_messages: RwLock::new(HashMap::new()),
            operator_rate_limits: RwLock::new(HashMap::new()),
            session_chat,
            knowledge_engine,
            shared_knowledge_watcher: RwLock::new(None),
            ghost_knowledge_watchers: RwLock::new(HashMap::new()),
            heartbeat_runner: RwLock::new(None),
            heartbeat_overrides: RwLock::new(HashMap::new()),
            scheduler: RwLock::new(SchedulerState::new()),
            discord_bot_token: RwLock::new(None),
        }
    }

    /// Toggle verbose tool call visibility for an operator (persisted in DB).
    pub async fn set_verbose(&self, operator_id: &str, enabled: bool) {
        if let Err(e) =
            t_koma_db::OperatorRepository::set_verbose(self.koma_db.pool(), operator_id, enabled)
                .await
        {
            warn!("failed to persist verbose setting for {operator_id}: {e}");
        }
    }

    /// Check if an operator has verbose tool call visibility.
    pub async fn is_verbose(&self, operator_id: &str) -> bool {
        match t_koma_db::OperatorRepository::get_by_id(self.koma_db.pool(), operator_id).await {
            Ok(Some(op)) => op.verbose,
            _ => false,
        }
    }

    pub async fn set_discord_bot_token(&self, token: Option<String>) {
        let mut guard = self.discord_bot_token.write().await;
        *guard = token;
    }

    pub async fn discord_bot_token(&self) -> Option<String> {
        let guard = self.discord_bot_token.read().await;
        guard.clone()
    }

    pub async fn set_heartbeat_override(
        &self,
        key: &str,
        next_due: i64,
        last_seen_updated_at: i64,
    ) {
        let mut guard = self.heartbeat_overrides.write().await;
        guard.insert(
            key.to_string(),
            HeartbeatOverride {
                next_due,
                last_seen_updated_at,
            },
        );
    }

    pub async fn clear_heartbeat_override(&self, key: &str) {
        let mut guard = self.heartbeat_overrides.write().await;
        guard.remove(key);
    }

    pub async fn get_heartbeat_override(&self, key: &str) -> Option<HeartbeatOverride> {
        let guard = self.heartbeat_overrides.read().await;
        guard.get(key).copied()
    }

    pub async fn set_heartbeat_due(&self, key: &str, next_due: Option<i64>) {
        let mut guard = self.scheduler.write().await;
        guard.set_due(JobKind::Heartbeat, key, next_due);
    }

    pub async fn get_heartbeat_due(&self, key: &str) -> Option<i64> {
        let guard = self.scheduler.read().await;
        guard.get_due(JobKind::Heartbeat, key)
    }

    /// Generic scheduler access for any job kind.
    pub async fn scheduler_set(&self, kind: JobKind, key: &str, next_due: Option<i64>) {
        let mut guard = self.scheduler.write().await;
        guard.set_due(kind, key, next_due);
    }

    /// Generic scheduler read for any job kind.
    pub async fn scheduler_get(&self, kind: JobKind, key: &str) -> Option<i64> {
        let guard = self.scheduler.read().await;
        guard.get_due(kind, key)
    }

    /// List all scheduler entries (for admin/TUI display).
    pub async fn scheduler_state(&self) -> Vec<(JobKind, String, i64)> {
        let guard = self.scheduler.read().await;
        guard.list_all()
    }

    /// Start the heartbeat runner if it isn't already running.
    pub async fn start_heartbeat_runner(
        self: &Arc<Self>,
        heartbeat_model_alias: Option<String>,
        timing: t_koma_core::HeartbeatTimingSettings,
    ) {
        let mut guard = self.heartbeat_runner.write().await;
        if let Some(handle) = guard.as_ref()
            && !handle.is_finished()
        {
            return;
        }

        let handle = crate::heartbeat::start_heartbeat_runner(
            Arc::clone(self),
            heartbeat_model_alias,
            timing,
        );
        *guard = Some(handle);
    }

    /// Access the knowledge engine.
    pub fn knowledge_engine(&self) -> &t_koma_knowledge::KnowledgeEngine {
        &self.knowledge_engine
    }

    /// Access the knowledge settings (from the engine).
    pub fn knowledge_settings(&self) -> &t_koma_knowledge::KnowledgeSettings {
        self.knowledge_engine.settings()
    }

    /// Get the default model entry
    pub fn default_model(&self) -> &ModelEntry {
        self.models
            .get(&self.default_model_alias)
            .expect("default model alias must exist")
    }

    /// Get a model entry by alias
    pub fn get_model_by_alias(&self, alias: &str) -> Option<&ModelEntry> {
        self.models.get(alias)
    }

    /// Get a model entry by provider name and model id
    pub fn get_model_by_provider_and_id(
        &self,
        provider: &str,
        model_id: &str,
    ) -> Option<&ModelEntry> {
        self.models
            .values()
            .find(|entry| entry.provider == provider && entry.model == model_id)
    }

    /// List configured models for a provider
    pub fn list_models_for_provider(&self, provider: &str) -> Vec<t_koma_core::ModelInfo> {
        let mut models: Vec<t_koma_core::ModelInfo> = self
            .models
            .values()
            .filter(|entry| entry.provider == provider)
            .map(|entry| t_koma_core::ModelInfo {
                id: entry.model.clone(),
                name: entry.alias.clone(),
                description: Some(format!("{} ({})", entry.model, entry.provider)),
                context_length: None,
            })
            .collect();

        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    /// Get a receiver for log entries
    pub fn subscribe_logs(&self) -> broadcast::Receiver<LogEntry> {
        self.log_tx.subscribe()
    }

    /// Broadcast a log entry
    pub async fn log(&self, entry: LogEntry) {
        let _ = self.log_tx.send(entry);
    }

    /// Restart the gateway process by spawning a replacement process and exiting.
    pub async fn restart_gateway(&self) -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

        std::process::Command::new(&executable)
            .args(args)
            .spawn()
            .map_err(|e| e.to_string())?;

        self.log(LogEntry::Info {
            message: "Gateway restart requested via WebSocket".to_string(),
        })
        .await;

        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            std::process::exit(0);
        });

        Ok(())
    }

    /// Send a chat message and get the AI response
    ///
    /// This is a convenience method that delegates to `session_chat.chat()`.
    /// All conversation logic (history, tools, system prompts) is handled internally.
    ///
    /// # Arguments
    /// * `ghost_name` - The GHOST name to chat with
    /// * `session_id` - The session ID to chat in
    /// * `operator_id` - The operator ID (for session ownership verification)
    /// * `message` - The operator's message content
    ///
    /// # Returns
    /// The final text response from the provider
    pub async fn chat(
        &self,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        message: &str,
    ) -> Result<String, ChatError> {
        let result = self
            .chat_detailed(ghost_name, session_id, operator_id, message, None)
            .await?;
        Ok(result.text)
    }

    /// Send a chat message and get the AI response plus metadata.
    ///
    /// If `tool_call_tx` is provided, tool call summaries are streamed
    /// incrementally during the tool loop.
    pub async fn chat_detailed(
        &self,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        message: &str,
        tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
    ) -> Result<ChatResult, ChatError> {
        let chat_key = Self::chat_key(operator_id, ghost_name, session_id);
        if self.is_chat_in_flight(&chat_key).await {
            if !Self::is_continue_message(message) {
                self.set_ignored_message(&chat_key, message).await;
            }
            return Ok(ChatResult {
                text: render_message(ids::CHAT_BUSY, &[]),
                compaction_happened: false,
                tool_calls: Vec::new(),
            });
        }

        let message = if Self::is_continue_message(message) {
            match self.take_ignored_message(&chat_key).await {
                Some(ignored) => ignored,
                None => {
                    return Ok(ChatResult {
                        text: render_message(ids::CHAT_CONTINUE_MISSING, &[]),
                        compaction_happened: false,
                        tool_calls: Vec::new(),
                    });
                }
            }
        } else {
            message.to_string()
        };

        self.log(LogEntry::OperatorMessage {
            operator_id: operator_id.to_string(),
            ghost_name: ghost_name.to_string(),
            content: message.clone(),
        })
        .await;

        let model = self.default_model();
        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.koma_db.pool(), ghost_name).await {
                Ok(Some(g)) => g,
                Ok(None) => {
                    return Err(ChatError::Database(DbError::GhostNotFound(
                        ghost_name.to_string(),
                    )));
                }
                Err(e) => return Err(ChatError::Database(e)),
            };
        let pre_compaction_state =
            t_koma_db::SessionRepository::get_by_id(self.koma_db.pool(), session_id).await?;
        self.set_chat_in_flight(&chat_key).await;

        let workspace_path = t_koma_db::ghosts::ghost_workspace_path(&ghost.name)?;
        if let Err(err) = crate::heartbeat::ensure_heartbeat_file(&workspace_path) {
            tracing::warn!(
                "Failed to ensure HEARTBEAT.md for ghost {}: {}",
                ghost_name,
                err
            );
        }
        self.ensure_ghost_watcher(ghost_name).await;

        let response = self
            .session_chat
            .chat(
                &self.koma_db,
                &ghost.id,
                model.client.as_ref(),
                &model.provider,
                &model.model,
                model.context_window,
                session_id,
                operator_id,
                &message,
                tool_call_tx,
            )
            .await;
        self.clear_chat_in_flight(&chat_key).await;

        let (text, tool_calls) = match response {
            Ok(pair) => pair,
            Err(ChatError::EmptyResponse) => {
                let message_preview = if message.len() > 240 {
                    format!("{}...", &message[..message.floor_char_boundary(240)])
                } else {
                    message.clone()
                };
                warn!(
                    event_kind = "chat_io",
                    operator_id = operator_id,
                    ghost_name = ghost_name,
                    session_id = session_id,
                    provider = model.provider.as_str(),
                    model = model.model.as_str(),
                    message_preview = message_preview.as_str(),
                    "empty final response from provider"
                );
                return Err(ChatError::EmptyResponse);
            }
            Err(err) => return Err(err),
        };
        let post_compaction_state =
            t_koma_db::SessionRepository::get_by_id(self.koma_db.pool(), session_id).await?;

        self.log(LogEntry::GhostMessage {
            ghost_name: ghost_name.to_string(),
            content: text.clone(),
        })
        .await;

        let compaction_happened = match (pre_compaction_state, post_compaction_state) {
            (Some(before), Some(after)) => {
                before.compaction_cursor_id != after.compaction_cursor_id
                    || before.compaction_summary != after.compaction_summary
            }
            _ => false,
        };

        Ok(ChatResult {
            text,
            compaction_happened,
            tool_calls,
        })
    }

    /// Send a chat message using a specific model alias
    pub async fn chat_with_model_alias(
        &self,
        model_alias: &str,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        message: &str,
    ) -> Result<String, ChatError> {
        let result = self
            .chat_with_model_alias_detailed(
                model_alias,
                ghost_name,
                session_id,
                operator_id,
                message,
                None,
            )
            .await?;
        Ok(result.text)
    }

    /// Send a chat message using a specific model alias and return metadata.
    pub async fn chat_with_model_alias_detailed(
        &self,
        model_alias: &str,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        message: &str,
        tool_call_tx: Option<&tokio::sync::mpsc::UnboundedSender<Vec<ToolCallSummary>>>,
    ) -> Result<ChatResult, ChatError> {
        let chat_key = Self::chat_key(operator_id, ghost_name, session_id);
        if self.is_chat_in_flight(&chat_key).await {
            if !Self::is_continue_message(message) {
                self.set_ignored_message(&chat_key, message).await;
            }
            return Ok(ChatResult {
                text: render_message(ids::CHAT_BUSY, &[]),
                compaction_happened: false,
                tool_calls: Vec::new(),
            });
        }

        let message = if Self::is_continue_message(message) {
            match self.take_ignored_message(&chat_key).await {
                Some(ignored) => ignored,
                None => {
                    return Ok(ChatResult {
                        text: render_message(ids::CHAT_CONTINUE_MISSING, &[]),
                        compaction_happened: false,
                        tool_calls: Vec::new(),
                    });
                }
            }
        } else {
            message.to_string()
        };

        self.log(LogEntry::OperatorMessage {
            operator_id: operator_id.to_string(),
            ghost_name: ghost_name.to_string(),
            content: message.clone(),
        })
        .await;

        let model = self
            .models
            .get(model_alias)
            .unwrap_or_else(|| self.default_model());

        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.koma_db.pool(), ghost_name).await {
                Ok(Some(g)) => g,
                Ok(None) => {
                    return Err(ChatError::Database(DbError::GhostNotFound(
                        ghost_name.to_string(),
                    )));
                }
                Err(e) => return Err(ChatError::Database(e)),
            };
        let pre_compaction_state =
            t_koma_db::SessionRepository::get_by_id(self.koma_db.pool(), session_id).await?;
        self.set_chat_in_flight(&chat_key).await;

        let workspace_path = t_koma_db::ghosts::ghost_workspace_path(&ghost.name)?;
        if let Err(err) = crate::heartbeat::ensure_heartbeat_file(&workspace_path) {
            tracing::warn!(
                "Failed to ensure HEARTBEAT.md for ghost {}: {}",
                ghost_name,
                err
            );
        }
        self.ensure_ghost_watcher(ghost_name).await;

        let response = self
            .session_chat
            .chat(
                &self.koma_db,
                &ghost.id,
                model.client.as_ref(),
                &model.provider,
                &model.model,
                model.context_window,
                session_id,
                operator_id,
                &message,
                tool_call_tx,
            )
            .await;
        self.clear_chat_in_flight(&chat_key).await;

        let (text, tool_calls) = match response {
            Ok(pair) => pair,
            Err(ChatError::EmptyResponse) => {
                let message_preview = if message.len() > 240 {
                    format!("{}...", &message[..message.floor_char_boundary(240)])
                } else {
                    message.clone()
                };
                warn!(
                    event_kind = "chat_io",
                    operator_id = operator_id,
                    ghost_name = ghost_name,
                    session_id = session_id,
                    provider = model.provider.as_str(),
                    model = model.model.as_str(),
                    model_alias = model_alias,
                    message_preview = message_preview.as_str(),
                    "empty final response from provider"
                );
                return Err(ChatError::EmptyResponse);
            }
            Err(err) => return Err(err),
        };
        let post_compaction_state =
            t_koma_db::SessionRepository::get_by_id(self.koma_db.pool(), session_id).await?;

        self.log(LogEntry::GhostMessage {
            ghost_name: ghost_name.to_string(),
            content: text.clone(),
        })
        .await;

        let compaction_happened = match (pre_compaction_state, post_compaction_state) {
            (Some(before), Some(after)) => {
                before.compaction_cursor_id != after.compaction_cursor_id
                    || before.compaction_summary != after.compaction_summary
            }
            _ => false,
        };

        Ok(ChatResult {
            text,
            compaction_happened,
            tool_calls,
        })
    }

    /// Start the shared knowledge watcher if not already running.
    pub async fn start_shared_knowledge_watcher(&self) {
        let mut guard = self.shared_knowledge_watcher.write().await;
        if let Some(handle) = guard.as_ref()
            && !handle.is_finished()
        {
            return;
        }

        let settings = self.knowledge_engine.settings().clone();
        let handle = tokio::spawn(async move {
            let mut backoff = 2u64;
            loop {
                let result = t_koma_knowledge::watcher::run_shared_watcher(settings.clone()).await;
                if let Err(err) = result {
                    error!("shared knowledge watcher crashed: {err}");
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        });

        *guard = Some(handle);
    }

    async fn ensure_ghost_watcher(&self, ghost_name: &str) {
        let mut guard = self.ghost_knowledge_watchers.write().await;
        if let Some(handle) = guard.get(ghost_name)
            && !handle.is_finished()
        {
            return;
        }

        let settings = self.knowledge_engine.settings().clone();
        let ghost_name_key = ghost_name.to_string();
        let ghost_name_log = ghost_name_key.clone();
        let ghost_name_task = ghost_name_key.clone();
        let handle = tokio::spawn(async move {
            let mut backoff = 2u64;
            loop {
                let result = t_koma_knowledge::watcher::run_ghost_watcher(
                    settings.clone(),
                    ghost_name_task.clone(),
                )
                .await;
                if let Err(err) = result {
                    error!(
                        "ghost knowledge watcher crashed ({}): {err}",
                        ghost_name_log
                    );
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        });

        guard.insert(ghost_name_key, handle);
    }

    /// Set the active ghost for an operator
    pub async fn set_active_ghost(&self, operator_id: &str, ghost_name: &str) {
        let mut guard = self.active_ghosts.write().await;
        guard.insert(operator_id.to_string(), ghost_name.to_string());
    }

    /// Get the active ghost name for an operator
    pub async fn get_active_ghost(&self, operator_id: &str) -> Option<String> {
        let guard = self.active_ghosts.read().await;
        guard.get(operator_id).cloned()
    }

    fn interface_key(platform: t_koma_db::Platform, external_id: &str) -> String {
        format!("{}:{}", platform, external_id)
    }

    fn approval_key(operator_id: &str, ghost_name: &str, session_id: &str) -> String {
        format!("{}:{}:{}", operator_id, ghost_name, session_id)
    }

    fn chat_key(operator_id: &str, ghost_name: &str, session_id: &str) -> String {
        format!("{}:{}:{}", operator_id, ghost_name, session_id)
    }

    fn is_continue_message(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("continue")
    }

    pub async fn is_chat_in_flight(&self, key: &str) -> bool {
        let guard = self.in_flight_chats.read().await;
        guard.contains(key)
    }

    pub async fn set_chat_in_flight(&self, key: &str) {
        let mut guard = self.in_flight_chats.write().await;
        guard.insert(key.to_string());
    }

    pub async fn clear_chat_in_flight(&self, key: &str) {
        let mut guard = self.in_flight_chats.write().await;
        guard.remove(key);
    }

    pub async fn set_ignored_message(&self, key: &str, message: &str) {
        let mut guard = self.ignored_messages.write().await;
        guard.insert(key.to_string(), message.to_string());
    }

    pub async fn take_ignored_message(&self, key: &str) -> Option<String> {
        let mut guard = self.ignored_messages.write().await;
        guard.remove(key)
    }

    pub async fn store_pending_message(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
        message: &str,
    ) {
        let key = Self::chat_key(operator_id, ghost_name, session_id);
        self.set_ignored_message(&key, message).await;
    }

    pub async fn check_operator_rate_limit(
        &self,
        operator: &t_koma_db::Operator,
    ) -> RateLimitDecision {
        if operator.access_level == t_koma_db::OperatorAccessLevel::PuppetMaster {
            return RateLimitDecision::Allowed;
        }

        if operator.rate_limit_5m_max.is_none() && operator.rate_limit_1h_max.is_none() {
            return RateLimitDecision::Allowed;
        }

        let now = Utc::now().timestamp();
        let mut guard = self.operator_rate_limits.write().await;
        let state = guard
            .entry(operator.id.clone())
            .or_insert_with(OperatorRateLimitState::default);

        let cutoff_5m = now - 300;
        while let Some(&timestamp) = state.last_5m.front() {
            if timestamp <= cutoff_5m {
                state.last_5m.pop_front();
            } else {
                break;
            }
        }

        let cutoff_1h = now - 3600;
        while let Some(&timestamp) = state.last_1h.front() {
            if timestamp <= cutoff_1h {
                state.last_1h.pop_front();
            } else {
                break;
            }
        }

        let mut retry_after_secs = 0i64;
        if let Some(max) = operator.rate_limit_5m_max
            && (state.last_5m.len() as i64) >= max
            && let Some(&oldest) = state.last_5m.front()
        {
            retry_after_secs = retry_after_secs.max(oldest + 300 - now);
        }

        if let Some(max) = operator.rate_limit_1h_max
            && (state.last_1h.len() as i64) >= max
            && let Some(&oldest) = state.last_1h.front()
        {
            retry_after_secs = retry_after_secs.max(oldest + 3600 - now);
        }

        if retry_after_secs > 0 {
            return RateLimitDecision::Limited {
                retry_after: Duration::from_secs(retry_after_secs as u64),
            };
        }

        if operator.rate_limit_5m_max.is_some() {
            state.last_5m.push_back(now);
        }
        if operator.rate_limit_1h_max.is_some() {
            state.last_1h.push_back(now);
        }

        RateLimitDecision::Allowed
    }

    pub async fn is_interface_pending(
        &self,
        platform: t_koma_db::Platform,
        external_id: &str,
    ) -> bool {
        let key = Self::interface_key(platform, external_id);
        let guard = self.pending_interfaces.read().await;
        guard.contains_key(&key)
    }

    pub async fn set_interface_pending(&self, platform: t_koma_db::Platform, external_id: &str) {
        let key = Self::interface_key(platform, external_id);
        let mut guard = self.pending_interfaces.write().await;
        guard.insert(
            key,
            PendingInterface {
                platform,
                external_id: external_id.to_string(),
            },
        );
    }

    pub async fn clear_interface_pending(&self, platform: t_koma_db::Platform, external_id: &str) {
        let key = Self::interface_key(platform, external_id);
        let mut guard = self.pending_interfaces.write().await;
        guard.remove(&key);
    }

    pub async fn set_pending_tool_approval(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
        pending: PendingToolApproval,
    ) {
        let key = Self::approval_key(operator_id, ghost_name, session_id);
        let mut guard = self.pending_tool_approvals.write().await;
        guard.insert(key, pending);
    }

    pub async fn take_pending_tool_approval(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
    ) -> Option<PendingToolApproval> {
        let key = Self::approval_key(operator_id, ghost_name, session_id);
        let mut guard = self.pending_tool_approvals.write().await;
        guard.remove(&key)
    }

    pub async fn set_pending_tool_loop(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
        pending: PendingToolContinuation,
    ) {
        let key = Self::approval_key(operator_id, ghost_name, session_id);
        let mut guard = self.pending_tool_loops.write().await;
        guard.insert(key, pending);
    }

    pub async fn take_pending_tool_loop(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
    ) -> Option<PendingToolContinuation> {
        let key = Self::approval_key(operator_id, ghost_name, session_id);
        let mut guard = self.pending_tool_loops.write().await;
        guard.remove(&key)
    }

    pub async fn clear_pending_tool_loop(
        &self,
        operator_id: &str,
        ghost_name: &str,
        session_id: &str,
    ) -> bool {
        let key = Self::approval_key(operator_id, ghost_name, session_id);
        let mut guard = self.pending_tool_loops.write().await;
        guard.remove(&key).is_some()
    }

    pub async fn set_pending_gateway_action(&self, token: &str, pending: PendingGatewayAction) {
        let mut guard = self.pending_gateway_actions.write().await;
        guard.insert(token.to_string(), pending);
    }

    pub async fn take_pending_gateway_action(&self, token: &str) -> Option<PendingGatewayAction> {
        let now = Utc::now().timestamp();
        let mut guard = self.pending_gateway_actions.write().await;
        guard.retain(|_, action| action.expires_at > now);
        guard.remove(token)
    }

    pub async fn handle_tool_approval(
        &self,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        decision: ToolApprovalDecision,
        model_alias: Option<&str>,
    ) -> Result<Option<String>, ChatError> {
        let pending = match self
            .take_pending_tool_approval(operator_id, ghost_name, session_id)
            .await
        {
            Some(pending) => pending,
            None => return Ok(None),
        };

        let model = model_alias
            .and_then(|alias| self.get_model_by_alias(alias))
            .unwrap_or_else(|| self.default_model());

        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.koma_db.pool(), ghost_name).await {
                Ok(Some(g)) => g,
                Ok(None) => {
                    return Err(ChatError::Database(DbError::GhostNotFound(
                        ghost_name.to_string(),
                    )));
                }
                Err(e) => return Err(ChatError::Database(e)),
            };
        let response = self
            .session_chat
            .resume_tool_approval(
                &self.koma_db,
                &ghost.id,
                model.client.as_ref(),
                &model.provider,
                &model.model,
                model.context_window,
                session_id,
                operator_id,
                pending,
                decision,
            )
            .await?;

        Ok(Some(response))
    }

    pub async fn handle_tool_loop_continue(
        &self,
        ghost_name: &str,
        session_id: &str,
        operator_id: &str,
        extra_iterations: Option<usize>,
        model_alias: Option<&str>,
    ) -> Result<Option<String>, ChatError> {
        let pending = match self
            .take_pending_tool_loop(operator_id, ghost_name, session_id)
            .await
        {
            Some(pending) => pending,
            None => return Ok(None),
        };

        let model = model_alias
            .and_then(|alias| self.get_model_by_alias(alias))
            .unwrap_or_else(|| self.default_model());

        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.koma_db.pool(), ghost_name).await {
                Ok(Some(g)) => g,
                Ok(None) => {
                    return Err(ChatError::Database(DbError::GhostNotFound(
                        ghost_name.to_string(),
                    )));
                }
                Err(e) => return Err(ChatError::Database(e)),
            };
        let response = self
            .session_chat
            .resume_tool_loop(
                &self.koma_db,
                &ghost.id,
                model.client.as_ref(),
                &model.provider,
                &model.model,
                model.context_window,
                session_id,
                operator_id,
                pending,
                extra_iterations.unwrap_or(DEFAULT_TOOL_LOOP_EXTRA),
            )
            .await?;

        Ok(Some(response))
    }

    /// Low-level conversation method with full tool use loop support
    ///
    /// This is primarily intended for integration tests that need explicit control
    /// over the conversation flow. Normal interfaces should use `chat()` instead.
    /// Send a conversation with full tool use loop support
    ///
    /// This is the main entry point for AI conversations - it handles:
    /// 1. Sending the conversation to the AI
    /// 2. Detecting if the AI wants to use tools
    /// 3. Executing tools and sending results back
    /// 4. Returning the final text response
    ///
    /// # Arguments
    /// * `provider` - The provider to use
    /// * `session_id` - The session ID for logging
    /// * `system_blocks` - System prompt blocks with optional cache control
    /// * `api_messages` - Conversation history in API format
    /// * `tools` - Available tools for the AI to use
    /// * `new_message` - Optional new user message to add
    /// * `model` - Model name for saving responses
    ///
    /// # Returns
    /// The final text response from the provider after all tool use is complete
    #[cfg(feature = "live-tests")]
    /// The final text response from the AI after all tool use is complete
    #[allow(clippy::too_many_arguments)]
    pub async fn send_conversation_with_tools(
        &self,
        ghost_name: &str,
        provider: &dyn Provider,
        session_id: &str,
        system_blocks: Vec<crate::prompt::render::SystemBlock>,
        api_messages: Vec<crate::chat::history::ChatMessage>,
        tools: Vec<&dyn crate::tools::Tool>,
        new_message: Option<&str>,
        model: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use crate::chat::history::build_history_messages;
        use crate::providers::provider::has_tool_uses;
        use t_koma_db::SessionRepository;
        use tracing::info;

        let ghost =
            match t_koma_db::GhostRepository::get_by_name(self.koma_db.pool(), ghost_name).await {
                Ok(Some(g)) => g,
                Ok(None) => return Err(format!("Ghost '{}' not found", ghost_name).into()),
                Err(e) => return Err(e.into()),
            };

        // Initial request to AI
        let mut response = provider
            .send_conversation(
                Some(system_blocks.clone()),
                api_messages.clone(),
                tools.clone(),
                new_message,
                None,
                None,
            )
            .await?;

        // Handle tool use loop (max 5 iterations to prevent infinite loops)
        for iteration in 0..5 {
            let has_tool_use = has_tool_uses(&response);

            if !has_tool_use {
                break;
            }

            info!(
                "[session:{}] Tool use detected (iteration {})",
                session_id,
                iteration + 1
            );

            // Save assistant message with tool_use blocks
            self.save_assistant_response(&self.koma_db, &ghost.id, session_id, model, &response)
                .await;

            // Execute tools and get results
            let tool_results = self
                .execute_tools_from_response(session_id, &response)
                .await;

            // Save tool results to database
            self.save_tool_results(&self.koma_db, &ghost.id, session_id, &tool_results)
                .await;

            // Build new API messages including the tool results
            let history = SessionRepository::get_messages(self.koma_db.pool(), session_id).await?;
            let new_api_messages = build_history_messages(&history, None);

            // Send tool results back to AI
            response = provider
                .send_conversation(
                    Some(system_blocks.clone()),
                    new_api_messages,
                    tools.clone(),
                    None,
                    None,
                    None,
                )
                .await?;
        }

        // Extract and save final text response
        let text = self
            .finalize_response(
                &self.koma_db,
                &ghost.id,
                session_id,
                provider.name(),
                model,
                &response,
            )
            .await;

        Ok(text)
    }

    /// Save an assistant response (with tool_use blocks) to the database
    #[cfg(feature = "live-tests")]
    async fn save_assistant_response(
        &self,
        pool: &t_koma_db::KomaDbPool,
        ghost_id: &str,
        session_id: &str,
        model: &str,
        response: &ProviderResponse,
    ) {
        use t_koma_db::{ContentBlock as DbContentBlock, MessageRole, SessionRepository};

        let assistant_content: Vec<DbContentBlock> = response
            .content
            .iter()
            .map(|block| match block {
                crate::providers::provider::ProviderContentBlock::Text { text } => {
                    DbContentBlock::Text { text: text.clone() }
                }
                crate::providers::provider::ProviderContentBlock::ToolUse { id, name, input } => {
                    DbContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
                crate::providers::provider::ProviderContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => DbContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                },
            })
            .collect();

        if let Err(e) = SessionRepository::add_message(
            pool.pool(),
            ghost_id,
            session_id,
            MessageRole::Ghost,
            assistant_content,
            Some(model),
        )
        .await
        {
            error!(
                "[session:{}] Failed to save assistant message: {}",
                session_id, e
            );
        }
    }

    /// Execute all tool_use blocks from a response and return the results
    #[cfg(feature = "live-tests")]
    async fn execute_tools_from_response(
        &self,
        session_id: &str,
        response: &ProviderResponse,
    ) -> Vec<t_koma_db::ContentBlock> {
        use t_koma_db::ContentBlock as DbContentBlock;

        let mut tool_results = Vec::new();

        for block in &response.content {
            let crate::providers::provider::ProviderContentBlock::ToolUse { id, name, input } =
                block
            else {
                continue;
            };

            info!(
                "[session:{}] Executing tool: {} (id: {})",
                session_id, name, id
            );

            let result = self
                .execute_tool_by_name(name.as_str(), input.clone())
                .await;

            let content = match result {
                Ok(output) => output,
                Err(e) => format!("Error: {}", e),
            };

            tool_results.push(DbContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content,
                is_error: None,
            });
        }

        tool_results
    }

    /// Execute a tool by name with the given input
    #[cfg(feature = "live-tests")]
    async fn execute_tool_by_name(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<String, String> {
        let mut context = ToolContext::new(
            "live-test".to_string(),
            std::path::PathBuf::from("/"),
            std::path::PathBuf::from("/"),
            true,
        );
        self.session_chat
            .tool_manager
            .execute_with_context(name, input, &mut context)
            .await
    }

    /// Save tool results to the database
    #[cfg(feature = "live-tests")]
    async fn save_tool_results(
        &self,
        pool: &t_koma_db::KomaDbPool,
        ghost_id: &str,
        session_id: &str,
        tool_results: &[t_koma_db::ContentBlock],
    ) {
        use t_koma_db::{MessageRole, SessionRepository};

        if let Err(e) = SessionRepository::add_message(
            pool.pool(),
            ghost_id,
            session_id,
            MessageRole::Operator,
            tool_results.to_vec(),
            None,
        )
        .await
        {
            error!(
                "[session:{}] Failed to save tool results: {}",
                session_id, e
            );
        }
    }

    /// Extract final text response and save it to the database
    #[cfg(feature = "live-tests")]
    async fn finalize_response(
        &self,
        pool: &t_koma_db::KomaDbPool,
        ghost_id: &str,
        session_id: &str,
        provider_name: &str,
        model: &str,
        response: &ProviderResponse,
    ) -> String {
        use t_koma_db::SessionRepository;

        let text = extract_all_text(response);

        let preview = if text.len() > 100 {
            &text[..text.floor_char_boundary(100)]
        } else {
            &text
        };
        info!(
            "[session:{}] AI final response ({} / {}): {}",
            session_id, provider_name, model, preview
        );

        let final_content = vec![t_koma_db::ContentBlock::Text { text: text.clone() }];
        let _ = SessionRepository::add_message(
            pool.pool(),
            ghost_id,
            session_id,
            t_koma_db::MessageRole::Ghost,
            final_content,
            Some(model),
        )
        .await;

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_display() {
        let entry = LogEntry::DiscordMessage {
            channel: "general".to_string(),
            user: "alice".to_string(),
            content: "Hello!".to_string(),
        };
        let s = format!("{}", entry);
        assert!(s.contains("[DISCORD]"));
        assert!(s.contains("alice"));
        assert!(s.contains("Hello!"));
    }
}
