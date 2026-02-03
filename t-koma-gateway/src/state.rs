use tokio::sync::broadcast;
use tracing::{error, info};

use crate::models::anthropic::AnthropicClient;
use crate::tools::Tool;

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
    /// Anthropic API client
    pub anthropic: AnthropicClient,
    /// Log broadcast channel
    log_tx: broadcast::Sender<LogEntry>,
    /// Database pool
    pub db: t_koma_db::DbPool,
}

impl AppState {
    pub fn new(anthropic: AnthropicClient, db: t_koma_db::DbPool) -> Self {
        let (log_tx, _) = broadcast::channel(100);
        Self {
            anthropic,
            log_tx,
            db,
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

    /// Send a conversation to Claude with full tool use loop support
    ///
    /// This is the main entry point for AI conversations - it handles:
    /// 1. Sending the conversation to Claude
    /// 2. Detecting if Claude wants to use tools
    /// 3. Executing tools and sending results back
    /// 4. Returning the final text response
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
    pub async fn send_conversation_with_tools(
        &self,
        session_id: &str,
        system_blocks: Vec<crate::models::anthropic::prompt::SystemBlock>,
        api_messages: Vec<crate::models::anthropic::history::ApiMessage>,
        tools: Vec<&dyn crate::tools::Tool>,
        new_message: Option<&str>,
        model: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use crate::models::anthropic::{AnthropicClient, ContentBlock};
        use crate::models::anthropic::history::build_api_messages;
        use t_koma_db::{ContentBlock as DbContentBlock, MessageRole, SessionRepository};

        // Initial request
        let mut response = self.anthropic.send_conversation(
            Some(system_blocks.clone()),
            api_messages.clone(),
            tools.clone(),
            new_message,
            None,
            None,
        ).await?;

        // Handle tool use loop (max 5 iterations to prevent infinite loops)
        for iteration in 0..5 {
            let has_tool_use = response.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
            
            if !has_tool_use {
                // No tool use, we're done
                break;
            }

            info!("[session:{}] Tool use detected (iteration {})", session_id, iteration + 1);

            // Build content blocks for the assistant message (includes tool_use)
            let assistant_content: Vec<DbContentBlock> = response.content.iter().map(|block| {
                match block {
                    ContentBlock::Text { text } => DbContentBlock::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => DbContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                }
            }).collect();

            // Save assistant message with tool_use
            if let Err(e) = SessionRepository::add_message(
                self.db.pool(),
                session_id,
                MessageRole::Assistant,
                assistant_content,
                Some(model),
            ).await {
                error!("[session:{}] Failed to save assistant message: {}", session_id, e);
            }

            // Execute tools and build tool results
            let mut tool_results = Vec::new();
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    info!("[session:{}] Executing tool: {} (id: {})", session_id, name, id);
                    
                    // Find and execute the tool
                    let result = if name == "run_shell_command" {
                        let shell_tool = crate::tools::shell::ShellTool;
                        shell_tool.execute(input.clone()).await
                    } else {
                        Err(format!("Unknown tool: {}", name))
                    };

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
            }

            // Save tool results
            if let Err(e) = SessionRepository::add_message(
                self.db.pool(),
                session_id,
                MessageRole::User,
                tool_results.clone(),
                None,
            ).await {
                error!("[session:{}] Failed to save tool results: {}", session_id, e);
            }

            // Build new API messages including the tool results
            let history = SessionRepository::get_messages(self.db.pool(), session_id).await?;
            let new_api_messages = build_api_messages(&history, Some(50));

            // Send tool results back to Claude
            response = self.anthropic.send_conversation(
                Some(system_blocks.clone()),
                new_api_messages,
                tools.clone(),
                None, // No new user message, just continuing with tool results
                None,
                None,
            ).await?;
        }

        // Extract final text response
        let text = AnthropicClient::extract_all_text(&response);

        info!("[session:{}] Claude final response: {}",
            session_id,
            if text.len() > 100 { &text[..100] } else { &text });

        // Save final assistant response
        let final_content = vec![DbContentBlock::Text { text: text.clone() }];
        if let Err(e) = SessionRepository::add_message(
            self.db.pool(),
            session_id,
            MessageRole::Assistant,
            final_content,
            Some(model),
        ).await {
            error!("[session:{}] Failed to save final assistant message: {}", session_id, e);
        }

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
