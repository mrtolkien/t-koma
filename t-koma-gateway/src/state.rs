use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::models::provider::{
    extract_all_text, has_tool_uses, Provider, ProviderResponse,
};

/// Log entry for broadcasting events to listeners
#[derive(Debug, Clone)]
pub enum LogEntry {
    /// Discord message received
    DiscordMessage {
        channel: String,
        user: String,
        content: String,
    },
    /// AI response sent to Discord
    DiscordResponse {
        user: String,
        content: String,
    },
    /// HTTP request handled
    HttpRequest {
        method: String,
        path: String,
        status: u16,
    },
    /// WebSocket event
    WebSocket {
        event: String,
        client_id: String,
    },
    /// General info message
    Info {
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
                    format!("{}...", &content[..50])
                } else {
                    content.clone()
                }
            ),
            LogEntry::HttpRequest {
                method,
                path,
                status,
            } => write!(
                f,
                "[{}] [HTTP] {} {} {}",
                timestamp, method, path, status
            ),
            LogEntry::WebSocket { event, client_id } => write!(
                f,
                "[{}] [WS] {} {}",
                timestamp, event, client_id
            ),
            LogEntry::Info { message } => {
                write!(f, "[{}] [INFO] {}", timestamp, message)
            }
        }
    }
}

/// Shared application state
pub struct AppState {
    /// Default model alias
    default_model_alias: String,
    /// Model registry keyed by alias
    models: HashMap<String, ModelEntry>,
    /// Log broadcast channel
    log_tx: broadcast::Sender<LogEntry>,
    /// Database pool
    pub db: t_koma_db::DbPool,
}

/// Model entry tracked by the gateway
pub struct ModelEntry {
    pub alias: String,
    pub provider: String,
    pub model: String,
    pub client: Arc<dyn Provider>,
}

impl AppState {
    pub fn new(
        default_model_alias: String,
        models: HashMap<String, ModelEntry>,
        db: t_koma_db::DbPool,
    ) -> Self {
        let (log_tx, _) = broadcast::channel(100);
        Self {
            default_model_alias,
            models,
            log_tx,
            db,
        }
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
        self.models.values().find(|entry| {
            entry.provider == provider && entry.model == model_id
        })
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
    /// The final text response from the AI after all tool use is complete
    #[allow(clippy::too_many_arguments)]
    pub async fn send_conversation_with_tools(
        &self,
        provider: &dyn Provider,
        session_id: &str,
        system_blocks: Vec<crate::models::anthropic::prompt::SystemBlock>,
        api_messages: Vec<crate::models::anthropic::history::ApiMessage>,
        tools: Vec<&dyn crate::tools::Tool>,
        new_message: Option<&str>,
        model: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use crate::models::anthropic::history::build_api_messages;
        use t_koma_db::SessionRepository;

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
            self.save_assistant_response(session_id, model, &response)
                .await;

            // Execute tools and get results
            let tool_results = self.execute_tools_from_response(session_id, &response).await;

            // Save tool results to database
            self.save_tool_results(session_id, &tool_results).await;

            // Build new API messages including the tool results
            let history = SessionRepository::get_messages(self.db.pool(), session_id).await?;
            let new_api_messages = build_api_messages(&history, Some(50));

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
            .finalize_response(session_id, provider.name(), model, &response)
            .await;

        Ok(text)
    }

    /// Save an assistant response (with tool_use blocks) to the database
    async fn save_assistant_response(
        &self,
        session_id: &str,
        model: &str,
        response: &ProviderResponse,
    ) {
        use t_koma_db::{ContentBlock as DbContentBlock, MessageRole, SessionRepository};

        let assistant_content: Vec<DbContentBlock> = response
            .content
            .iter()
            .map(|block| match block {
                crate::models::provider::ProviderContentBlock::Text { text } => {
                    DbContentBlock::Text { text: text.clone() }
                }
                crate::models::provider::ProviderContentBlock::ToolUse { id, name, input } => {
                    DbContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
                crate::models::provider::ProviderContentBlock::ToolResult {
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
            self.db.pool(),
            session_id,
            MessageRole::Assistant,
            assistant_content,
            Some(model),
        )
        .await
        {
            error!("[session:{}] Failed to save assistant message: {}", session_id, e);
        }
    }

    /// Execute all tool_use blocks from a response and return the results
    async fn execute_tools_from_response(
        &self,
        session_id: &str,
        response: &ProviderResponse,
    ) -> Vec<t_koma_db::ContentBlock> {
        use t_koma_db::ContentBlock as DbContentBlock;

        let mut tool_results = Vec::new();

        for block in &response.content {
            let crate::models::provider::ProviderContentBlock::ToolUse { id, name, input } = block else {
                continue;
            };

            info!("[session:{}] Executing tool: {} (id: {})", session_id, name, id);

            let result = self.execute_tool_by_name(name.as_str(), input.clone()).await;

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
    async fn execute_tool_by_name(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<String, String> {
        use crate::tools::{file_edit::FileEditTool, shell::ShellTool, Tool};

        match name {
            "run_shell_command" => {
                let tool = ShellTool;
                tool.execute(input).await
            }
            "replace" => {
                let tool = FileEditTool;
                tool.execute(input).await
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }

    /// Save tool results to the database
    async fn save_tool_results(
        &self,
        session_id: &str,
        tool_results: &[t_koma_db::ContentBlock],
    ) {
        use t_koma_db::{MessageRole, SessionRepository};

        if let Err(e) = SessionRepository::add_message(
            self.db.pool(),
            session_id,
            MessageRole::User,
            tool_results.to_vec(),
            None,
        )
        .await
        {
            error!("[session:{}] Failed to save tool results: {}", session_id, e);
        }
    }

    /// Extract final text response and save it to the database
    async fn finalize_response(
        &self,
        session_id: &str,
        provider_name: &str,
        model: &str,
        response: &ProviderResponse,
    ) -> String {
        use t_koma_db::{ContentBlock as DbContentBlock, MessageRole, SessionRepository};

        let text = extract_all_text(response);

        let preview = if text.len() > 100 {
            &text[..100]
        } else {
            &text
        };
        info!(
            "[session:{}] AI final response ({} / {}): {}",
            session_id, provider_name, model, preview
        );

        let final_content = vec![DbContentBlock::Text { text: text.clone() }];
        if let Err(e) = SessionRepository::add_message(
            self.db.pool(),
            session_id,
            MessageRole::Assistant,
            final_content,
            Some(model),
        )
        .await
        {
            error!(
                "[session:{}] Failed to save final assistant message: {}",
                session_id, e
            );
        }

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
