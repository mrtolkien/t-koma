//! Interactive provider and model selection for the CLI.

use std::io::{self, Write};
use tokio::sync::mpsc;
use tracing::{error, info};

use t_koma_core::{GatewayMessageKind, ModelInfo, WsMessage, WsResponse, message::ProviderType};

/// Provider and model selection result
#[derive(Debug, Clone)]
pub struct ProviderSelection {
    pub provider: ProviderType,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSelectionMode {
    /// Send provider selection to the gateway (chat mode).
    SendToGateway,
    /// Local-only selection (config mode).
    LocalOnly,
}

/// Run the interactive provider selection flow
pub async fn select_provider_interactive(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    select_provider_interactive_with_mode(ws_tx, ws_rx, ProviderSelectionMode::SendToGateway).await
}

/// Run the interactive provider selection flow with a configurable mode
pub async fn select_provider_interactive_with_mode(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    mode: ProviderSelectionMode,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    println!("\n╔════════════════════════════════════╗");
    println!("║     Select Model Provider          ║");
    println!("╠════════════════════════════════════╣");
    println!("║  1. Anthropic                      ║");
    println!("║  2. OpenRouter                     ║");
    println!("║  3. llama.cpp                      ║");
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [1-3]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    match input.trim() {
        "1" => {
            info!("User selected Anthropic provider");
            select_anthropic_model(ws_tx, ws_rx, mode).await
        }
        "2" => {
            info!("User selected OpenRouter provider");
            select_openrouter_model(ws_tx, ws_rx, mode).await
        }
        "3" => {
            info!("User selected llama.cpp provider");
            select_llama_cpp_model(ws_tx, ws_rx, mode).await
        }
        _ => {
            println!("Invalid selection, defaulting to Anthropic");
            select_anthropic_model(ws_tx, ws_rx, mode).await
        }
    }
}

/// Select an Anthropic model (configured list)
async fn select_anthropic_model(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    mode: ProviderSelectionMode,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    // Request available models from gateway
    ws_tx.send(WsMessage::ListAvailableModels {
        provider: ProviderType::Anthropic,
    })?;

    // Wait for response
    let models = wait_for_models(ws_rx, "anthropic").await?;
    if models.is_empty() {
        return Err("No Anthropic models configured".into());
    }

    println!("\n╔════════════════════════════════════╗");
    println!("║     Select Anthropic Model         ║");
    println!("╠════════════════════════════════════╣");

    for (i, model) in models.iter().enumerate().take(10) {
        println!("║  {}. {:<30} ║", i + 1, truncate(&model.name, 28));
        if let Some(desc) = &model.description {
            println!("║     {:<32} ║", truncate(desc, 30));
        }
    }
    println!("║  0. Enter custom model ID          ║");
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [0-{}]: ", models.len().min(10));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let selection: usize = input.trim().parse().unwrap_or(1);

    let model_id = if selection == 0 {
        print!("Enter custom model ID: ");
        io::stdout().flush()?;
        let mut custom = String::new();
        io::stdin().read_line(&mut custom)?;
        custom.trim().to_string()
    } else if selection <= models.len() {
        models[selection - 1].id.clone()
    } else {
        models.first().map(|m| m.id.clone()).unwrap_or_default()
    };

    if model_id.is_empty() {
        return Err("Model ID cannot be empty".into());
    }

    if mode == ProviderSelectionMode::SendToGateway {
        // Send selection to gateway
        ws_tx.send(WsMessage::SelectProvider {
            provider: ProviderType::Anthropic,
            model: model_id.clone(),
        })?;

        // Wait for confirmation
        wait_for_provider_confirmation(ws_rx).await?;

        println!("✓ Selected Anthropic model: {}", model_id);
    }

    Ok(ProviderSelection {
        provider: ProviderType::Anthropic,
        model: model_id,
    })
}

/// Select an OpenRouter model (fetched from API)
async fn select_openrouter_model(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    mode: ProviderSelectionMode,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    println!("\nFetching available models from OpenRouter...");

    // Request available models from gateway
    ws_tx.send(WsMessage::ListAvailableModels {
        provider: ProviderType::OpenRouter,
    })?;

    // Wait for response
    let models = wait_for_models(ws_rx, "openrouter").await?;

    if models.is_empty() {
        println!("No models fetched. Enter custom model ID.");
        return select_custom_openrouter_model(ws_tx, ws_rx, mode).await;
    }

    println!("\n╔════════════════════════════════════╗");
    println!("║     Select OpenRouter Model        ║");
    println!("╠════════════════════════════════════╣");
    println!("║  Configured models (first 20):     ║");

    for (i, model) in models.iter().enumerate().take(20) {
        let name = truncate(&model.name, 28);
        println!("║  {:2}. {:<30} ║", i + 1, name);
    }

    if models.len() > 20 {
        println!("║     ... and {} more models          ║", models.len() - 20);
    }

    println!("║  0. Enter custom model ID          ║");
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [0-{}]: ", models.len().min(20));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let selection: usize = input.trim().parse().unwrap_or(1);

    let model_id = if selection == 0 {
        return select_custom_openrouter_model(ws_tx, ws_rx, mode).await;
    } else if selection <= models.len() {
        models[selection - 1].id.clone()
    } else {
        models.first().map(|m| m.id.clone()).unwrap_or_default()
    };

    if model_id.is_empty() {
        return Err("Model ID cannot be empty".into());
    }

    if mode == ProviderSelectionMode::SendToGateway {
        // Send selection to gateway
        ws_tx.send(WsMessage::SelectProvider {
            provider: ProviderType::OpenRouter,
            model: model_id.clone(),
        })?;

        // Wait for confirmation
        wait_for_provider_confirmation(ws_rx).await?;

        println!("✓ Selected OpenRouter model: {}", model_id);
    }

    Ok(ProviderSelection {
        provider: ProviderType::OpenRouter,
        model: model_id,
    })
}

/// Select a llama.cpp model (configured list)
async fn select_llama_cpp_model(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    mode: ProviderSelectionMode,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    ws_tx.send(WsMessage::ListAvailableModels {
        provider: ProviderType::LlamaCpp,
    })?;

    let models = wait_for_models(ws_rx, "llama_cpp").await?;
    if models.is_empty() {
        return Err("No llama.cpp models configured".into());
    }

    println!("\n╔════════════════════════════════════╗");
    println!("║      Select llama.cpp Model        ║");
    println!("╠════════════════════════════════════╣");

    for (i, model) in models.iter().enumerate().take(10) {
        println!("║  {}. {:<30} ║", i + 1, truncate(&model.name, 28));
        if let Some(desc) = &model.description {
            println!("║     {:<32} ║", truncate(desc, 30));
        }
    }
    println!("╚════════════════════════════════════╝");
    print!("\nSelect [1-{}]: ", models.len().min(10));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let selection: usize = input.trim().parse().unwrap_or(1);
    let model_id = if selection == 0 {
        models.first().map(|m| m.id.clone()).unwrap_or_default()
    } else if selection <= models.len() {
        models[selection - 1].id.clone()
    } else {
        models.first().map(|m| m.id.clone()).unwrap_or_default()
    };

    if model_id.is_empty() {
        return Err("Model ID cannot be empty".into());
    }

    if mode == ProviderSelectionMode::SendToGateway {
        ws_tx.send(WsMessage::SelectProvider {
            provider: ProviderType::LlamaCpp,
            model: model_id.clone(),
        })?;

        wait_for_provider_confirmation(ws_rx).await?;

        println!("✓ Selected llama.cpp model: {}", model_id);
    }

    Ok(ProviderSelection {
        provider: ProviderType::LlamaCpp,
        model: model_id,
    })
}

/// Enter a custom OpenRouter model ID
async fn select_custom_openrouter_model(
    ws_tx: &mpsc::UnboundedSender<WsMessage>,
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    mode: ProviderSelectionMode,
) -> Result<ProviderSelection, Box<dyn std::error::Error>> {
    print!("Enter OpenRouter model ID: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let model_id = input.trim().to_string();

    if model_id.is_empty() {
        return Err("Model ID cannot be empty".into());
    }

    if mode == ProviderSelectionMode::SendToGateway {
        // Send selection to gateway
        ws_tx.send(WsMessage::SelectProvider {
            provider: ProviderType::OpenRouter,
            model: model_id.clone(),
        })?;

        // Wait for confirmation
        wait_for_provider_confirmation(ws_rx).await?;

        println!("✓ Selected OpenRouter model: {}", model_id);
    }

    Ok(ProviderSelection {
        provider: ProviderType::OpenRouter,
        model: model_id,
    })
}

/// Wait for available models response
async fn wait_for_models(
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
    expected_provider: &str,
) -> Result<Vec<ModelInfo>, Box<dyn std::error::Error>> {
    use tokio::time::{Duration, timeout};

    loop {
        match timeout(Duration::from_secs(30), ws_rx.recv()).await {
            Ok(Some(WsResponse::AvailableModels { provider, models })) => {
                if provider == expected_provider {
                    return Ok(models);
                } else {
                    return Err(format!("Unexpected provider: {}", provider).into());
                }
            }
            Ok(Some(WsResponse::Response { message, .. }))
                if message.kind == GatewayMessageKind::Error =>
            {
                error!("Gateway error fetching models: {}", message.text_fallback);
                return Err(message.text_fallback.into());
            }
            Ok(Some(_)) => {
                // Unexpected message type, continue waiting
                continue;
            }
            Ok(None) => return Err("WebSocket closed".into()),
            Err(_) => {
                error!("Timeout waiting for models response");
                return Err("Timeout waiting for models response".into());
            }
        }
    }
}

/// Wait for provider selection confirmation
async fn wait_for_provider_confirmation(
    ws_rx: &mut mpsc::UnboundedReceiver<WsResponse>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::time::{Duration, timeout};

    loop {
        match timeout(Duration::from_secs(10), ws_rx.recv()).await {
            Ok(Some(WsResponse::ProviderSelected { .. })) => return Ok(()),
            Ok(Some(WsResponse::Response { message, .. }))
                if message.kind == GatewayMessageKind::Error =>
            {
                return Err(
                    format!("Provider selection failed: {}", message.text_fallback).into()
                );
            }
            Ok(Some(_)) => {
                // Unexpected message type, continue waiting
                continue;
            }
            Ok(None) => return Err("WebSocket closed".into()),
            Err(_) => return Err("Timeout waiting for provider confirmation".into()),
        }
    }
}

/// Truncate a string to a maximum length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
