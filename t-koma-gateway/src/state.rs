use tokio::sync::broadcast;

use crate::models::anthropic::AnthropicClient;
use crate::session::{ChatError, SessionChat};

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
            } => write!(f, "[{}] [HTTP] {} {} {}", timestamp, method, path, status),
            LogEntry::WebSocket { event, client_id } => {
                write!(f, "[{}] [WS] {} {}", timestamp, event, client_id)
            }
            LogEntry::Info { message } => {
                write!(f, "[{}] [INFO] {}", timestamp, message)
            }
        }
    }
}

/// Shared application state
///
/// This holds all shared resources and provides the main interface for
/// handling chat conversations through `session_chat`.
pub struct AppState {
    /// Anthropic API client
    pub anthropic: AnthropicClient,
    /// Log broadcast channel
    log_tx: broadcast::Sender<LogEntry>,
    /// Database pool
    pub db: t_koma_db::DbPool,
    /// High-level chat interface - handles all conversation logic including tools
    pub session_chat: SessionChat,
}

impl AppState {
    /// Create a new AppState with the given Anthropic client and database
    pub fn new(anthropic: AnthropicClient, db: t_koma_db::DbPool) -> Self {
        let (log_tx, _) = broadcast::channel(100);
        let session_chat = SessionChat::new(db.clone(), anthropic.clone());

        Self {
            anthropic,
            log_tx,
            db,
            session_chat,
        }
    }

    /// Get a receiver for log entries
    pub fn subscribe_logs(&self) -> broadcast::Receiver<LogEntry> {
        self.log_tx.subscribe()
    }

    /// Broadcast a log entry
    pub async fn log(&self, entry: LogEntry) {
        let _ = self.log_tx.send(entry);
    }

    /// Send a chat message and get the AI response
    ///
    /// This is a convenience method that delegates to `session_chat.chat()`.
    /// All conversation logic (history, tools, system prompts) is handled internally.
    ///
    /// # Arguments
    /// * `session_id` - The session ID to chat in
    /// * `user_id` - The user ID (for session ownership verification)
    /// * `message` - The user's message content
    ///
    /// # Returns
    /// The final text response from Claude
    pub async fn chat(
        &self,
        session_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<String, ChatError> {
        self.session_chat.chat(session_id, user_id, message).await
    }

    /// Low-level conversation method with full tool use loop support
    ///
    /// This is primarily intended for integration tests that need explicit control
    /// over the conversation flow. Normal interfaces should use `chat()` instead.
    ///
    /// # Arguments
    /// * `session_id` - The session ID for logging
    /// * `system_blocks` - System prompt blocks with optional cache control
    /// * `api_messages` - Conversation history in API format
    /// * `tools` - Available tools for Claude to use
    /// * `new_message` - Optional new user message to add
    /// * `model` - Model name for saving responses
    ///
    /// # Returns
    /// The final text response from Claude after all tool use is complete
    #[cfg(feature = "live-tests")]
    pub async fn send_conversation_with_tools(
        &self,
        session_id: &str,
        system_blocks: Vec<crate::models::anthropic::prompt::SystemBlock>,
        api_messages: Vec<crate::models::anthropic::history::ApiMessage>,
        tools: Vec<&dyn crate::tools::Tool>,
        new_message: Option<&str>,
        model: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use crate::models::anthropic::history::build_api_messages;
        use t_koma_db::SessionRepository;
        use tracing::info;

        // Initial request to Claude
        let mut response = self
            .anthropic
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
            let has_tool_use = response
                .content
                .iter()
                .any(|b| matches!(b, crate::models::anthropic::ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                break;
            }

            info!(
                "[session:{}] Tool use detected (iteration {})",
                session_id,
                iteration + 1
            );

            // Save assistant message with tool_use blocks
            let assistant_content: Vec<t_koma_db::ContentBlock> = response
                .content
                .iter()
                .map(|block| match block {
                    crate::models::anthropic::ContentBlock::Text { text } => {
                        t_koma_db::ContentBlock::Text { text: text.clone() }
                    }
                    crate::models::anthropic::ContentBlock::ToolUse { id, name, input } => {
                        t_koma_db::ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        }
                    }
                })
                .collect();

            let _ = SessionRepository::add_message(
                self.db.pool(),
                session_id,
                t_koma_db::MessageRole::Assistant,
                assistant_content,
                Some(model),
            )
            .await;

            // Execute tools and get results
            let mut tool_results = Vec::new();
            for block in &response.content {
                let crate::models::anthropic::ContentBlock::ToolUse { id, name, input } = block
                else {
                    continue;
                };

                info!(
                    "[session:{}] Executing tool: {} (id: {})",
                    session_id, name, id
                );

                let result = self
                    .session_chat
                    .tool_manager
                    .execute(name, input.clone())
                    .await;

                let content = match result {
                    Ok(output) => output,
                    Err(e) => format!("Error: {}", e),
                };

                tool_results.push(t_koma_db::ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content,
                    is_error: None,
                });
            }

            // Save tool results to database
            let _ = SessionRepository::add_message(
                self.db.pool(),
                session_id,
                t_koma_db::MessageRole::User,
                tool_results,
                None,
            )
            .await;

            // Build new API messages including the tool results
            let history = SessionRepository::get_messages(self.db.pool(), session_id).await?;
            let new_api_messages = build_api_messages(&history, Some(50));

            // Send tool results back to Claude
            response = self
                .anthropic
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
        let text = crate::models::anthropic::AnthropicClient::extract_all_text(&response);

        info!(
            "[session:{}] Claude final response: {}",
            session_id,
            if text.len() > 100 {
                &text[..100]
            } else {
                &text
            }
        );

        let final_content = vec![t_koma_db::ContentBlock::Text { text: text.clone() }];
        let _ = SessionRepository::add_message(
            self.db.pool(),
            session_id,
            t_koma_db::MessageRole::Assistant,
            final_content,
            Some(model),
        )
        .await;

        Ok(text)
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
