use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Check if the gateway is already running at the given URL
async fn is_gateway_running(ws_url: &str) -> bool {
    // Extract host and port from ws:// URL
    let http_url = ws_url.replace("ws://", "http://").replace("/ws", "/health");

    match reqwest::get(&http_url).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

/// Spawn the gateway as a child process if not already running
pub async fn ensure_gateway_running(
    ws_url: &str,
) -> Result<Option<tokio::process::Child>, GatewaySpawnError> {
    // First check if gateway is already running
    if is_gateway_running(ws_url).await {
        info!("Gateway is already running at {}", ws_url);
        return Ok(None);
    }

    info!("Gateway not running, attempting to start...");

    // Try to spawn the gateway
    let child = spawn_gateway().await?;

    // Wait for gateway to be ready (up to 10 seconds)
    for i in 0..50 {
        sleep(Duration::from_millis(200)).await;
        if is_gateway_running(ws_url).await {
            info!("Gateway started successfully after {} attempts", i + 1);
            return Ok(Some(child));
        }
    }

    // If we get here, gateway didn't start properly
    error!("Gateway failed to start within timeout");
    Err(GatewaySpawnError::StartupTimeout)
}

/// Spawn the gateway binary as a child process
async fn spawn_gateway() -> Result<tokio::process::Child, GatewaySpawnError> {
    // Try to find the gateway binary
    let possible_paths = [
        "./target/release/t-koma-gateway".to_string(),
        "./target/debug/t-koma-gateway".to_string(),
        "t-koma-gateway".to_string(), // Assume it's in PATH
    ];

    for path in &possible_paths {
        info!("Trying to spawn gateway from: {}", path);

        let result = Command::new(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();

        match result {
            Ok(child) => {
                info!("Spawned gateway process (PID: {:?})", child.id());
                return Ok(child);
            }
            Err(e) => {
                warn!("Failed to spawn from {}: {}", path, e);
            }
        }
    }

    error!("Could not spawn gateway from any known location");
    Err(GatewaySpawnError::SpawnFailed)
}

/// Errors that can occur when spawning the gateway
#[derive(Debug, thiserror::Error)]
pub enum GatewaySpawnError {
    #[error("Failed to spawn gateway process")]
    SpawnFailed,
    #[error("Gateway failed to start within timeout")]
    StartupTimeout,
}
