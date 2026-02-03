//! Secrets configuration loaded from environment variables only.
//!
//! This module handles sensitive configuration like API keys that should
//! never be stored in files. All secrets are read from environment variables.

use std::env;

/// Secrets loaded exclusively from environment variables.
/// 
/// These are sensitive values that should never be written to disk
/// or committed to version control.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    /// Anthropic API key (env: ANTHROPIC_API_KEY)
    pub anthropic_api_key: Option<String>,
    
    /// OpenRouter API key (env: OPENROUTER_API_KEY)
    pub openrouter_api_key: Option<String>,
    
    /// Discord bot token (env: DISCORD_BOT_TOKEN)
    pub discord_bot_token: Option<String>,
}

/// Errors that can occur when loading secrets
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("Missing required secret: {0}")]
    MissingSecret(String),
    
    #[error("No provider API key configured. Set ANTHROPIC_API_KEY or OPENROUTER_API_KEY")]
    NoProviderConfigured,
}

impl Secrets {
    /// Load secrets from environment variables.
    ///
    /// This function also loads .env file if present (for development),
    /// but production should rely on actual environment variables.
    pub fn from_env() -> Result<Self, SecretsError> {
        // Load .env file if present (development convenience)
        let _ = dotenvy::dotenv();
        
        Self::from_env_inner()
    }
    
    /// Internal method to load from environment without loading .env
    pub(crate) fn from_env_inner() -> Result<Self, SecretsError> {
        let secrets = Self {
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").ok(),
            openrouter_api_key: env::var("OPENROUTER_API_KEY").ok(),
            discord_bot_token: env::var("DISCORD_BOT_TOKEN").ok(),
        };
        
        // Validate that at least one provider is configured
        if secrets.anthropic_api_key.is_none() && secrets.openrouter_api_key.is_none() {
            return Err(SecretsError::NoProviderConfigured);
        }
        
        Ok(secrets)
    }
    
    /// Check if a specific provider is available
    pub fn has_provider(&self, provider: &str) -> bool {
        match provider {
            "anthropic" => self.anthropic_api_key.is_some(),
            "openrouter" => self.openrouter_api_key.is_some(),
            _ => false,
        }
    }
    
    /// Get the available providers
    pub fn available_providers(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if self.anthropic_api_key.is_some() {
            providers.push("anthropic");
        }
        if self.openrouter_api_key.is_some() {
            providers.push("openrouter");
        }
        providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    
    // Use a mutex to ensure tests that modify environment variables don't run concurrently
    static ENV_MUTEX: Mutex<()> = Mutex::new(());
    
    fn clear_env() {
        unsafe {
            env::remove_var("ANTHROPIC_API_KEY");
            env::remove_var("OPENROUTER_API_KEY");
            env::remove_var("DISCORD_BOT_TOKEN");
        }
    }
    
    #[test]
    fn test_secrets_from_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe { env::set_var("ANTHROPIC_API_KEY", "sk-test"); }
        
        let secrets = Secrets::from_env().unwrap();
        assert_eq!(secrets.anthropic_api_key, Some("sk-test".to_string()));
    }
    
    #[test]
    fn test_load_anthropic_only() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe { env::set_var("ANTHROPIC_API_KEY", "sk-test"); }
        
        let secrets = Secrets::from_env_inner().unwrap();
        assert_eq!(secrets.anthropic_api_key, Some("sk-test".to_string()));
        assert!(secrets.openrouter_api_key.is_none());
        assert!(secrets.has_provider("anthropic"));
        assert!(!secrets.has_provider("openrouter"));
    }
    
    #[test]
    fn test_load_openrouter_only() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe { env::set_var("OPENROUTER_API_KEY", "sk-or-test"); }
        
        let secrets = Secrets::from_env_inner().unwrap();
        assert_eq!(secrets.openrouter_api_key, Some("sk-or-test".to_string()));
        assert!(secrets.anthropic_api_key.is_none());
        assert!(secrets.has_provider("openrouter"));
    }
    
    #[test]
    fn test_load_both_providers() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        unsafe {
            env::set_var("ANTHROPIC_API_KEY", "sk-ant");
            env::set_var("OPENROUTER_API_KEY", "sk-or");
            env::set_var("DISCORD_BOT_TOKEN", "discord-token");
        }
        
        let secrets = Secrets::from_env_inner().unwrap();
        assert!(secrets.anthropic_api_key.is_some());
        assert!(secrets.openrouter_api_key.is_some());
        assert_eq!(secrets.discord_bot_token, Some("discord-token".to_string()));
        
        let providers = secrets.available_providers();
        assert_eq!(providers.len(), 2);
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"openrouter"));
    }
    
    #[test]
    fn test_no_provider_error() {
        let _lock = ENV_MUTEX.lock().unwrap();
        clear_env();
        
        let result = Secrets::from_env_inner();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecretsError::NoProviderConfigured));
    }
}
