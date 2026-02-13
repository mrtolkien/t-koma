//! Model configuration helpers for the CLI.

use std::io::{self, Write};
use std::str::FromStr;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType},
};

use t_koma_core::ModelConfig;
use t_koma_core::{ModelAliases, Settings};
use t_koma_core::message::ProviderType;

use crate::provider_selection::ProviderSelection;

/// Print currently configured models.
pub fn print_models(settings: &Settings) {
    println!("Current models:");
    if settings.models.is_empty() {
        println!("  (none configured)");
        return;
    }

    for (alias, model) in &settings.models {
        let default_marker = if settings.default_model.first() == Some(alias.as_str()) {
            " (default)"
        } else {
            ""
        };
        println!(
            "  {} -> {}/{}{}",
            alias, model.provider, model.model, default_marker
        );
    }
}

/// Apply a provider/model selection (from gateway list) and set default model.
pub fn apply_gateway_selection(
    settings: &mut Settings,
    selection: ProviderSelection,
) -> Result<String, Box<dyn std::error::Error>> {
    let provider = selection.provider;
    let model_id = selection.model;

    if let Some((alias, _)) = settings
        .models
        .iter()
        .find(|(_, entry)| entry.provider == provider && entry.model == model_id)
    {
        settings.default_model = ModelAliases::single(alias.clone());
        return Ok(alias.clone());
    }

    let alias = prompt_alias(Some(&suggest_alias_from_model_id(&model_id)))?;
    let entry = ModelConfig {
        provider,
        model: model_id,
        base_url: None,
        api_key_env: None,
        routing: None,
        context_window: None,
        headers: None,
        retry_on_empty: None,
    };

    settings.models.insert(alias.clone(), entry);
    settings.default_model = ModelAliases::single(alias.clone());

    Ok(alias)
}

/// Configure models locally without a gateway connection.
pub fn configure_models_local(
    settings: &mut Settings,
) -> Result<String, Box<dyn std::error::Error>> {
    print_models(settings);

    if settings.models.is_empty() {
        println!("No models configured yet.");
        let alias = add_or_update_model(settings, None)?;
        settings.default_model = ModelAliases::single(alias.clone());
        return Ok(alias);
    }

    let current_default = settings
        .default_model
        .first()
        .unwrap_or("(unset)")
        .to_string();

    loop {
        print!(
            "Enter alias to set default, 'new' to add/update model, or Enter to keep '{}': ",
            current_default
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            return Ok(current_default.clone());
        }

        if input.eq_ignore_ascii_case("new") {
            let alias = add_or_update_model(settings, None)?;
            if prompt_yes_no("Set this model as default?", false)? {
                settings.default_model = ModelAliases::single(alias.clone());
            }
            return Ok(settings.default_model.first().unwrap_or("").to_string());
        }

        if settings.models.contains_key(input) {
            settings.default_model = ModelAliases::single(input);
            return Ok(input.to_string());
        }

        println!("Unknown alias '{}'. Try again.", input);
    }
}

fn add_or_update_model(
    settings: &mut Settings,
    suggested_alias: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    let existing_alias =
        suggested_alias.and_then(|alias| settings.models.get(alias).map(|_| alias.to_string()));
    let existing = existing_alias
        .as_ref()
        .and_then(|alias| settings.models.get(alias).cloned());
    if let Some(existing) = &existing {
        println!(
            "Updating '{}': currently {}/{}",
            existing_alias.as_deref().unwrap_or("model"),
            existing.provider,
            existing.model
        );
    }

    let provider = prompt_provider(existing.as_ref().map(|e| e.provider))?;
    let model = prompt_text("Model ID", existing.as_ref().map(|e| e.model.as_str()))?;
    let base_url = match provider {
        ProviderType::OpenAiCompatible => Some(prompt_text(
            "Base URL",
            existing.as_ref().and_then(|e| e.base_url.as_deref()),
        )?),
        ProviderType::OpenRouter => prompt_optional_text(
            "Base URL (optional)",
            existing.as_ref().and_then(|e| e.base_url.as_deref()),
        )?,
        ProviderType::Anthropic => None,
    };
    let api_key_env = match provider {
        ProviderType::OpenRouter | ProviderType::OpenAiCompatible => prompt_optional_text(
            "API key env var (optional)",
            existing.as_ref().and_then(|e| e.api_key_env.as_deref()),
        )?,
        ProviderType::Anthropic => None,
    };
    let routing = if provider == ProviderType::OpenRouter {
        let existing_routing = existing
            .as_ref()
            .and_then(|e| e.routing.as_ref())
            .map(|v| v.join(","));
        let routing_raw = prompt_optional_text(
            "Routing providers comma-separated (optional)",
            existing_routing.as_deref(),
        )?;
        routing_raw.map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
    } else {
        None
    };
    let suggested_alias = suggest_alias_from_model_id(&model);
    let alias = prompt_alias(existing_alias.as_deref().or(Some(suggested_alias.as_str())))?;

    if let Some(old_alias) = existing_alias
        && old_alias != alias
    {
        settings.models.remove(&old_alias);
    }

    settings.models.insert(
        alias.clone(),
        ModelConfig {
            provider,
            model,
            base_url,
            api_key_env,
            routing,
            context_window: None,
            headers: None,
            retry_on_empty: None,
        },
    );

    Ok(alias)
}

fn prompt_alias(suggested: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        match suggested {
            Some(alias) => print!("Model alias (Enter to keep '{}'): ", alias),
            None => print!("Model alias: "),
        }
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            if let Some(alias) = suggested {
                return Ok(alias.to_string());
            }
            println!("Alias cannot be empty.");
            continue;
        }

        return Ok(input.to_string());
    }
}

fn prompt_provider(
    default: Option<ProviderType>,
) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let providers = [
        ProviderType::Anthropic,
        ProviderType::OpenRouter,
        ProviderType::OpenAiCompatible,
    ];
    let default_index = default
        .and_then(|value| providers.iter().position(|p| *p == value))
        .unwrap_or(0);

    if let Ok(selection) = prompt_provider_picker(&providers, default_index) {
        return Ok(selection);
    }

    loop {
        match default {
            Some(value) => print!(
                "Provider (anthropic/openrouter/openai_compatible) [default: {}]: ",
                value
            ),
            None => print!("Provider (anthropic/openrouter/openai_compatible): "),
        }
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            if let Some(value) = default {
                return Ok(value);
            }
            println!("Provider is required.");
            continue;
        }

        if let Ok(provider) = ProviderType::from_str(input) {
            return Ok(provider);
        }

        println!("Unknown provider '{}'.", input);
    }
}

fn prompt_text(label: &str, default: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        match default {
            Some(value) => print!("{} [default: {}]: ", label, value),
            None => print!("{}: ", label),
        }
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            if let Some(value) = default {
                return Ok(value.to_string());
            }
            println!("{} is required.", label);
            continue;
        }

        return Ok(input.to_string());
    }
}

fn prompt_optional_text(
    label: &str,
    default: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    loop {
        match default {
            Some(value) => print!("{} [default: {} | '-' to clear]: ", label, value),
            None => print!("{} [Enter to skip]: ", label),
        }
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input == "-" {
            return Ok(None);
        }
        if input.is_empty() {
            return Ok(default.map(ToString::to_string));
        }
        return Ok(Some(input.to_string()));
    }
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    print!("{} {} ", prompt, suffix);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input.is_empty() {
        return Ok(default);
    }

    Ok(matches!(input.as_str(), "y" | "yes"))
}

fn suggest_alias_from_model_id(model_id: &str) -> String {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return "model".to_string();
    }

    trimmed
        .split('/')
        .next_back()
        .unwrap_or(trimmed)
        .replace(['.', '-'], "_")
}

fn prompt_provider_picker(
    providers: &[ProviderType],
    default_index: usize,
) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let mut index = default_index.min(providers.len().saturating_sub(1));
    let mut query = String::new();
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;

    let result = (|| {
        loop {
            execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
            println!("Select provider (use arrows, type to search, Enter to confirm):");
            if !query.is_empty() {
                println!("Search: {}", query);
            } else {
                println!("Search: (type to filter)");
            }
            println!();

            for (i, provider) in providers.iter().enumerate() {
                if !query.is_empty() && !provider.as_str().starts_with(&query) {
                    continue;
                }
                if i == index {
                    println!("> {}", provider.as_str());
                } else {
                    println!("  {}", provider.as_str());
                }
            }

            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match code {
                    KeyCode::Up => {
                        index = if index == 0 {
                            providers.len() - 1
                        } else {
                            index - 1
                        };
                    }
                    KeyCode::Down => {
                        index = (index + 1) % providers.len();
                    }
                    KeyCode::Enter => return Ok(providers[index]),
                    KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => {
                        return Err("Provider selection cancelled".into());
                    }
                    KeyCode::Char(c) => {
                        query.push(c);
                        if let Some(pos) = providers
                            .iter()
                            .position(|p| p.as_str().starts_with(&query))
                        {
                            index = pos;
                        }
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        if query.is_empty() {
                            index = default_index.min(providers.len().saturating_sub(1));
                        } else if let Some(pos) = providers
                            .iter()
                            .position(|p| p.as_str().starts_with(&query))
                        {
                            index = pos;
                        }
                    }
                    KeyCode::Esc => return Err("Provider selection cancelled".into()),
                    _ => {}
                }
            }
        }
    })();

    terminal::disable_raw_mode()?;
    execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    result
}
