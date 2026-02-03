use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use futures::StreamExt;
use std::io::{self, Write};
use tokio_tungstenite::connect_async;
use tracing::{error, info};

/// Log follower that displays T-KOMA logs in real-time
pub struct LogFollower {
    ws_url: String,
}

impl LogFollower {
    /// Create a new log follower
    pub fn new(ws_url: impl Into<String>) -> Self {
        let url = ws_url.into();
        // Replace chat ws with logs ws if needed
        let logs_url = if url.ends_with("/ws") {
            url.replace("/ws", "/logs")
        } else {
            url
        };
        Self { ws_url: logs_url }
    }

    /// Run the log follower
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Connecting to logs at {}", self.ws_url);
        
        // Connect directly to WebSocket
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        info!("Connected to T-KOMA logs");
        
        let (_write, mut read) = ws_stream.split();
        
        println!("Connected to T-KOMA logs. Press 'q' or Ctrl+C to quit.\n");
        
        // Enable raw mode for immediate key detection
        terminal::enable_raw_mode()?;
        
        let result = self.run_loop(&mut read).await;
        
        // Cleanup
        terminal::disable_raw_mode()?;
        let _ = execute!(io::stdout(), terminal::Clear(ClearType::All));
        
        println!("\nLog follower stopped.");
        
        result
    }
    
    /// Main loop processing log messages and keyboard input
    async fn run_loop(
        &self,
        read: &mut (impl futures::Stream<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::time::{sleep, Duration};
        
        loop {
            // Check for WebSocket messages
            tokio::select! {
                Some(msg) = read.next() => {
                    match msg {
                        Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                            // Logs are plain text lines
                            let line = text.as_str();
                            self.print_log_line(line);
                        }
                        Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                            error!("WebSocket closed by server");
                            break;
                        }
                        Err(e) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
                
                // Check for keyboard input
                _ = sleep(Duration::from_millis(50)) => {
                    if event::poll(Duration::from_millis(0))?
                        && let Event::Key(key) = event::read()?
                    {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                return Ok(());
                            }
                            KeyCode::Char('c') if key.modifiers == crossterm::event::KeyModifiers::CONTROL => {
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Print a log line with color coding based on the source
    fn print_log_line(&self, line: &str) {
        // Parse the log line to determine the source
        let color = if line.contains("[DISCORD]") {
            Some(Color::Cyan)
        } else if line.contains("[AI]") {
            Some(Color::Green)
        } else if line.contains("[HTTP]") {
            Some(Color::Yellow)
        } else if line.contains("[WS]") {
            Some(Color::Magenta)
        } else if line.contains("[ERROR]") {
            Some(Color::Red)
        } else {
            None
        };
        
        // Print with color
        let mut stdout = io::stdout();
        if let Some(color) = color {
            let _ = execute!(
                stdout,
                SetForegroundColor(color),
                Print(line),
                Print("\r\n"),
                ResetColor
            );
        } else {
            let _ = execute!(stdout, Print(line), Print("\r\n"));
        }
        
        let _ = stdout.flush();
    }
}
