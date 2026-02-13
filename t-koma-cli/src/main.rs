use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cron::Schedule;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tracing::{error, info, warn};

mod client;
mod tui;

use tui::app::TuiApp;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(cmd) = std::env::args().nth(1)
        && cmd == "cron-validate"
    {
        let target = std::env::args().nth(2).map(PathBuf::from);
        return run_cron_validate(target).await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::sink)
        .init();

    let settings = t_koma_core::Settings::load()?;
    info!("Settings loaded, using {}", settings.ws_url());

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

    run_cyberdeck().await
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out);
        } else if path.extension().and_then(|v| v.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

async fn run_cron_validate(target: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let target = target.unwrap_or_else(|| PathBuf::from("cron"));
    if !target.exists() {
        return Err(format!("Path does not exist: {}", target.display()).into());
    }
    let mut checked = 0usize;
    let mut failed = 0usize;
    let mut files = Vec::new();
    if target.is_file() {
        files.push(target);
    } else {
        collect_markdown_files(&target, &mut files);
    }

    for file in files {
        checked += 1;
        let raw = std::fs::read_to_string(&file)?;
        match t_koma_core::parse_cron_job_markdown(&file, &raw) {
            Ok(parsed) => {
                let schedule = format!("0 {}", parsed.schedule);
                if let Err(err) = Schedule::from_str(&schedule) {
                    failed += 1;
                    eprintln!("INVALID {}: {}", file.display(), err);
                } else {
                    println!("OK      {}", file.display());
                }
            }
            Err(err) => {
                failed += 1;
                eprintln!("INVALID {}: {}", file.display(), err);
            }
        }
    }

    if checked == 0 {
        println!("No CRON markdown files found.");
        return Ok(());
    }
    if failed > 0 {
        return Err(format!("CRON validation failed: {} invalid file(s)", failed).into());
    }
    println!("Validated {} CRON file(s).", checked);
    Ok(())
}

async fn run_cyberdeck() -> Result<(), Box<dyn std::error::Error>> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new().await;

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

    terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
