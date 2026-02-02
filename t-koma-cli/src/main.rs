use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tracing::{error, info, warn};

mod app;
mod client;
mod gateway_spawner;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load configuration
    let config = t_koma_core::Config::from_env()?;
    info!("Configuration loaded, connecting to {}", config.gateway_ws_url);

    // Try to auto-start gateway if not running
    let _gateway_process = match gateway_spawner::ensure_gateway_running(&config.gateway_ws_url).await {
        Ok(process) => {
            if process.is_some() {
                info!("Started gateway automatically");
            }
            process
        }
        Err(e) => {
            warn!("Could not auto-start gateway: {}", e);
            info!("Assuming gateway is managed externally or will be started manually");
            None
        }
    };

    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new(&config.gateway_ws_url);
    
    let result = match app.run(&mut terminal).await {
        Ok(()) => {
            info!("Application exited normally");
            Ok(())
        }
        Err(e) => {
            error!("Application error: {}", e);
            Err(e)
        }
    };

    // Restore terminal
    terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
