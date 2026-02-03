# PKL Configuration Refactoring Spec

## Overview
Refactor the configuration system to:
1. Keep secrets (API keys) in environment variables only
2. Store hierarchical configuration in PKL format
3. Save config files in XDG config directory (`~/.config/t-koma/`)
4. Use clear categorical organization

## Design Principles

### Separation of Concerns
- **Secrets**: Environment variables only (API keys, tokens)
- **Settings**: PKL config files (provider settings, gateway config, etc.)
- **Defaults**: Sensible defaults embedded in code, written to config file on first run

### Hierarchical Structure
```
~/.config/t-koma/
├── config.pkl          # Main configuration file
└── config.d/           # Optional: additional config fragments (future)
```

## PKL Configuration Schema

```pkl
// t-koma configuration file
// Located at: ~/.config/t-koma/config.pkl

/// Provider settings
provider {
  /// Default provider to use ("anthropic" or "openrouter")
  default = "anthropic"
  
  /// Anthropic provider configuration
  anthropic {
    /// Model to use (default: claude-sonnet-4-5-20250929)
    model = "claude-sonnet-4-5-20250929"
  }
  
  /// OpenRouter provider configuration
  openrouter {
    /// Default model when using OpenRouter
    model = "anthropic/claude-3.5-sonnet"
    
    /// HTTP Referer header for OpenRouter rankings (optional)
    httpReferer = null
    
    /// App name for OpenRouter rankings (optional)
    appName = null
  }
}

/// Gateway server configuration
gateway {
  /// Host to bind to (default: 127.0.0.1)
  host = "127.0.0.1"
  
  /// Port to listen on (default: 3000)
  port = 3000
  
  /// WebSocket URL for CLI connections
  /// If not specified, computed from host and port
  wsUrl = null
}

/// Discord bot configuration (optional)
discord {
  /// Whether Discord bot is enabled
  /// Note: Also requires DISCORD_BOT_TOKEN env var
  enabled = false
}

/// Logging configuration
logging {
  /// Log level (error, warn, info, debug, trace)
  level = "info"
  
  /// Whether to log to file
  fileEnabled = false
  
  /// Log file path (if fileEnabled is true)
  filePath = null
}
```

## Environment Variables (Secrets Only)

```bash
# Anthropic API key (required if using Anthropic provider)
ANTHROPIC_API_KEY=sk-ant-...

# OpenRouter API key (required if using OpenRouter provider)
OPENROUTER_API_KEY=sk-or-...

# Discord bot token (required if discord.enabled = true)
DISCORD_BOT_TOKEN=
```

## Rust Implementation

### New Module Structure

```
t-koma-core/src/
├── lib.rs
├── config/
│   ├── mod.rs         # Public exports
│   ├── secrets.rs     # Secrets from env vars
│   ├── settings.rs    # Settings from PKL files
│   ├── defaults.rs    # Default configuration values
│   └── xdg.rs         # XDG directories handling
```

### Types

```rust
/// Secrets loaded from environment variables only
#[derive(Debug, Clone)]
pub struct Secrets {
    pub anthropic_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub discord_bot_token: Option<String>,
}

/// Settings loaded from PKL configuration file
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub provider: ProviderSettings,
    pub gateway: GatewaySettings,
    pub discord: DiscordSettings,
    pub logging: LoggingSettings,
}

/// Combined configuration
pub struct Config {
    pub secrets: Secrets,
    pub settings: Settings,
}
```

## Migration Path

1. First run: Create default `~/.config/t-koma/config.pkl`
2. Load secrets from environment
3. Validate: At least one provider key must be set
4. Use settings from PKL file

## Backward Compatibility

- If PKL config doesn't exist, create it with defaults
- Environment variables remain the source of truth for secrets
- Old flat env-based config is deprecated but can be migrated

## Files to Modify

- `t-koma-core/Cargo.toml` - Add `rpkl` and `xdg` dependencies
- `t-koma-core/src/config.rs` - Refactor into module
- `t-koma-core/src/lib.rs` - Update exports
- `t-koma-gateway/src/main.rs` - Use new config API
- `t-koma-cli/src/main.rs` - Use new config API
- `.env.example` - Remove non-secret config
- `AGENTS.md` - Update configuration docs

## Testing

1. Unit tests for config loading
2. Test default config creation
3. Test XDG directory resolution
4. Test PKL parsing
