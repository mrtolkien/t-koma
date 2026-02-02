use std::env;

/// Load .env file if it exists (called automatically when using `from_env`)
pub fn load_dotenv() {
    // Silently ignore errors (file might not exist)
    let _ = dotenvy::dotenv();
}

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Anthropic API key
    pub anthropic_api_key: String,
    /// Anthropic model to use (default: claude-sonnet-4-5-20250929)
    pub anthropic_model: String,
    /// Gateway host (default: 127.0.0.1)
    pub gateway_host: String,
    /// Gateway port (default: 3000)
    pub gateway_port: u16,
    /// WebSocket URL for CLI to connect (default: ws://127.0.0.1:3000/ws)
    pub gateway_ws_url: String,
}

impl Config {
    /// Load configuration from environment variables
    /// 
    /// This function automatically loads a .env file from the project root if present.
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load .env file if present (only if not already set in env)
        load_dotenv();
        
        Self::from_env_inner()
    }
    
    /// Internal method to load from env without loading .env
    fn from_env_inner() -> Result<Self, ConfigError> {
        let anthropic_api_key = env::var("ANTHROPIC_API_KEY")
            .map_err(|_| ConfigError::MissingVar("ANTHROPIC_API_KEY".to_string()))?;

        Ok(Self {
            anthropic_api_key,
            anthropic_model: env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-sonnet-4-5-20250929".to_string()),
            gateway_host: env::var("GATEWAY_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            gateway_port: env::var("GATEWAY_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            gateway_ws_url: env::var("GATEWAY_WS_URL")
                .unwrap_or_else(|_| "ws://127.0.0.1:3000/ws".to_string()),
        })
    }

    /// Get the HTTP bind address for the gateway
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.gateway_host, self.gateway_port)
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingVar(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        // Clear any existing values first, then set test values
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
            env::remove_var("ANTHROPIC_MODEL");
            env::remove_var("GATEWAY_HOST");
            env::remove_var("GATEWAY_PORT");
            env::remove_var("GATEWAY_WS_URL");
            
            env::set_var("ANTHROPIC_API_KEY", "test-key");
        }

        let config = Config::from_env_inner().unwrap();

        assert_eq!(config.anthropic_api_key, "test-key");
        assert_eq!(config.anthropic_model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.gateway_host, "127.0.0.1");
        assert_eq!(config.gateway_port, 3000);
        assert_eq!(config.gateway_ws_url, "ws://127.0.0.1:3000/ws");
        assert_eq!(config.bind_addr(), "127.0.0.1:3000");
    }

    #[test]
    fn test_config_custom_values() {
        unsafe {
            env::set_var("ANTHROPIC_API_KEY", "sk-test");
            env::set_var("ANTHROPIC_MODEL", "claude-opus-4");
            env::set_var("GATEWAY_HOST", "0.0.0.0");
            env::set_var("GATEWAY_PORT", "8080");
            env::set_var("GATEWAY_WS_URL", "ws://localhost:8080/ws");
        }

        let config = Config::from_env_inner().unwrap();

        assert_eq!(config.anthropic_api_key, "sk-test");
        assert_eq!(config.anthropic_model, "claude-opus-4");
        assert_eq!(config.gateway_host, "0.0.0.0");
        assert_eq!(config.gateway_port, 8080);
        assert_eq!(config.bind_addr(), "0.0.0.0:8080");
    }

    #[test]
    fn test_config_missing_api_key() {
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
        }

        let result = Config::from_env_inner();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ANTHROPIC_API_KEY"));
    }
}
