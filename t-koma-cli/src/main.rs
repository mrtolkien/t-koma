use std::io::{self, Write};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tracing::{error, info, warn};

mod app;
mod client;
mod gateway_spawner;
mod log_follower;
mod ui;

use app::App;
use log_follower::LogFollower;

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

    // Show menu and get selection
    let selection = show_menu()?;

    match selection {
        1 => run_chat_mode(&config.gateway_ws_url).await,
        2 => run_log_mode(&config.gateway_ws_url).await,
        _ => {
            println!("Invalid selection");
            Ok(())
        }
    }
}

/// Show the main menu and return the user's selection
fn show_menu() -> Result<u32, Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║           t-koma CLI               ║");
    println!("╠════════════════════════════════════╣");
    println!("║  1. Chat with t-koma               ║");
    println!("║  2. Follow gateway logs            ║");
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [1-2]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().parse().unwrap_or(0))
}

/// Run the chat TUI mode
async fn run_chat_mode(ws_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new(ws_url);
    
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

/// Run the log follow mode
async fn run_log_mode(ws_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nConnecting to gateway logs...\n");
    
    let follower = LogFollower::new(ws_url);
    follower.run().await
}
