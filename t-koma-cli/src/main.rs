use std::io::{self, Write};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

mod admin;
mod app;
mod client;
mod gateway_spawner;
mod log_follower;
mod model_config;
mod provider_selection;
mod ui;

use app::App;
use client::WsClient;
use futures::StreamExt;
use log_follower::LogFollower;
use model_config::{apply_gateway_selection, configure_models_local, print_models};
use provider_selection::{
    ProviderSelectionMode, select_provider_interactive, select_provider_interactive_with_mode,
};
use t_koma_core::{Secrets, Settings, WsResponse};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load settings (no default model validation in CLI startup)
    let settings = t_koma_core::Settings::load()?;
    let ws_url = settings.ws_url();
    info!("Settings loaded, using {}", ws_url);

    // Verify localhost-only for security
    if settings.gateway.host != "127.0.0.1" && settings.gateway.host != "localhost" {
        warn!(
            "Gateway is configured to bind to {} - this may expose it to remote access!",
            settings.gateway.host
        );
        print!("Continue anyway? [y/N]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Show menu and get selection
    let selection = show_menu()?;

    match selection {
        1 => run_chat_mode(&ws_url).await,
        2 => run_log_mode(&ws_url).await,
        3 => admin::run_admin_mode().await,
        4 => run_provider_config_mode(&ws_url).await,
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
    println!("║  2. Follow T-KOMA logs             ║");
    println!("║  3. Manage operators (admin)       ║");
    println!("║  4. Manage model config            ║");
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [1-4]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().parse().unwrap_or(0))
}

/// Run the chat TUI mode
async fn run_chat_mode(ws_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ws_url = ws_url_for_cli(ws_url);
    // Try to auto-start gateway if not running (chat mode only)
    let _gateway_process = match gateway_spawner::ensure_gateway_running(&ws_url).await {
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

    // First, connect to WebSocket and do provider selection in normal mode
    println!("\nConnecting to T-KOMA...");

    let (ws_tx, ws_rx) = WsClient::connect(&ws_url).await?;

    // Create channels for communication
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Spawn task to forward WebSocket messages
    tokio::spawn(async move {
        let mut ws_rx = ws_rx;
        while let Some(msg) = ws_rx.next().await {
            if tx.send(msg).is_err() {
                break;
            }
        }
    });

    // Wait for session creation before provider selection
    println!("Waiting for session...");
    let mut session_ready = false;
    while !session_ready {
        match rx.recv().await {
            Some(WsResponse::InterfaceSelectionRequired { message }) => {
                println!("{}", message);
                print!("Select [new/existing]: ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let choice = input.trim().to_string();
                if ws_tx
                    .send(t_koma_core::WsMessage::SelectInterface { choice })
                    .is_err()
                {
                    println!("Failed to send interface choice.");
                    return Ok(());
                }
            }
            Some(WsResponse::SessionCreated { .. }) => {
                session_ready = true;
                println!("✓ Session created");
            }
            Some(WsResponse::Error { message }) => {
                println!("Error: {}", message);
                return Ok(());
            }
            Some(WsResponse::Response { content, .. }) => {
                if content.contains("Connected to T-KOMA") {
                    // Continue waiting for SessionCreated
                }
            }
            _ => {}
        }
    }

    // Run provider selection interactively
    println!("\n--- Provider Selection ---");
    let _provider_selection = match select_provider_interactive(&ws_tx, &mut rx).await {
        Ok(selection) => {
            info!(
                "Selected provider: {:?}, model: {}",
                selection.provider, selection.model
            );
            selection
        }
        Err(e) => {
            println!("Provider selection failed: {}", e);
            println!("Falling back to configured default model.");
            let settings = Settings::load()?;
            let model_config = settings
                .default_model_config()
                .ok_or("Default model is not configured")?;
            provider_selection::ProviderSelection {
                provider: model_config.provider,
                model: model_config.model.clone(),
            }
        }
    };

    println!("\nStarting chat interface...\n");

    // Now setup terminal and run TUI
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new_with_channels(ws_url, ws_tx, rx);

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
    println!("\nConnecting to T-KOMA logs...\n");

    let follower = LogFollower::new(ws_url);
    follower.run().await
}

fn ws_url_for_cli(ws_url: &str) -> String {
    match url::Url::parse(ws_url) {
        Ok(mut url) => {
            url.query_pairs_mut().append_pair("client", "cli");
            url.to_string()
        }
        Err(_) => ws_url.to_string(),
    }
}

/// Run the provider configuration mode
async fn run_provider_config_mode(ws_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║       Model Configuration          ║");
    println!("╚════════════════════════════════════╝\n");

    let mut settings = Settings::load()?;

    print_models(&settings);

    let selection = match WsClient::connect(&ws_url_for_cli(ws_url)).await {
        Ok((ws_tx, ws_rx)) => {
            println!("\nT-KOMA reachable. Loading configured models...");

            let (tx, mut rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                let mut ws_rx = ws_rx;
                while let Some(msg) = ws_rx.next().await {
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            });

            match select_provider_interactive_with_mode(
                &ws_tx,
                &mut rx,
                ProviderSelectionMode::LocalOnly,
            )
            .await
            {
                Ok(selection) => Some(selection),
                Err(e) => {
                    println!("Gateway selection failed: {}", e);
                    println!("Falling back to local config selection.");
                    None
                }
            }
        }
        Err(e) => {
            println!("T-KOMA not reachable: {}", e);
            println!("Using local config selection.");
            None
        }
    };

    let alias = match selection {
        Some(selection) => apply_gateway_selection(&mut settings, selection)?,
        None => configure_models_local(&mut settings)?,
    };
    settings.save()?;

    if settings
        .models
        .get(&alias)
        .map(|model| model.provider == t_koma_core::ProviderType::OpenRouter)
        .unwrap_or(false)
    {
        match Secrets::from_env() {
            Ok(secrets) => {
                if !secrets.has_provider_type(t_koma_core::ProviderType::OpenRouter) {
                    println!("Warning: OPENROUTER_API_KEY is not set. You can set it later.");
                }
            }
            Err(_) => {
                println!("Warning: Unable to verify OPENROUTER_API_KEY. You can set it later.");
            }
        }
    }

    println!("✓ Updated default model to '{}'", alias);

    Ok(())
}
