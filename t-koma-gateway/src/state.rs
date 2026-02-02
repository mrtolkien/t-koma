use tokio::sync::broadcast;

use crate::models::anthropic::AnthropicClient;

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
}

impl AppState {
    pub fn new(anthropic: AnthropicClient) -> Self {
        let (log_tx, _) = broadcast::channel(100);
        Self {
            anthropic,
            log_tx,
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
