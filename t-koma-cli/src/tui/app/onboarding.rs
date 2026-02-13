use t_koma_core::{ModelAliases, ModelConfig, ProviderType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OnboardingStep {
    Welcome,
    ChooseProvider,
    EnterApiKey,
    ConfigureModel,
    EmbeddingsChoice,
    EmbeddingsApiKey,
    DiscordChoice,
    Summary,
}

impl OnboardingStep {
    pub fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::ChooseProvider => 1,
            Self::EnterApiKey => 2,
            Self::ConfigureModel => 3,
            Self::EmbeddingsChoice => 4,
            Self::EmbeddingsApiKey => 5,
            Self::DiscordChoice => 6,
            Self::Summary => 7,
        }
    }

    pub fn total() -> usize {
        8
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OnboardingState {
    pub step: OnboardingStep,
    pub provider: Option<ProviderType>,
    pub api_key: Option<String>,
    pub model_alias: String,
    pub model_id: String,
    pub embedding_provider: EmbeddingChoice,
    pub openai_embedding_key: Option<String>,
    pub discord_enabled: bool,
    pub input_buffer: String,
    pub selection_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbeddingChoice {
    Ollama,
    OpenAi,
    Skip,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            step: OnboardingStep::Welcome,
            provider: None,
            api_key: None,
            model_alias: String::new(),
            model_id: String::new(),
            embedding_provider: EmbeddingChoice::Ollama,
            openai_embedding_key: None,
            discord_enabled: false,
            input_buffer: String::new(),
            selection_idx: 0,
        }
    }
}

impl OnboardingState {
    pub fn provider_choices() -> &'static [(&'static str, ProviderType)] {
        &[
            ("Anthropic (Claude)", ProviderType::Anthropic),
            ("OpenRouter (multi-model)", ProviderType::OpenRouter),
            ("Google Gemini", ProviderType::Gemini),
            ("OpenAI Compatible", ProviderType::OpenAiCompatible),
            ("Kimi Code", ProviderType::KimiCode),
        ]
    }

    pub fn embedding_choices() -> &'static [(&'static str, EmbeddingChoice)] {
        &[
            ("Ollama (local, free)", EmbeddingChoice::Ollama),
            ("OpenAI (remote, paid)", EmbeddingChoice::OpenAi),
            ("Skip for now", EmbeddingChoice::Skip),
        ]
    }

    pub fn env_var_for_provider(provider: ProviderType) -> &'static str {
        match provider {
            ProviderType::Anthropic => "ANTHROPIC_API_KEY",
            ProviderType::OpenRouter => "OPENROUTER_API_KEY",
            ProviderType::Gemini => "GEMINI_API_KEY",
            ProviderType::OpenAiCompatible => "OPENAI_API_KEY",
            ProviderType::KimiCode => "KIMI_API_KEY",
        }
    }

    pub fn api_key_instructions(provider: ProviderType) -> &'static str {
        match provider {
            ProviderType::Anthropic => {
                "\
Go to https://console.anthropic.com/
Navigate to Settings > API Keys
Create a new key and copy it"
            }
            ProviderType::OpenRouter => {
                "\
Go to https://openrouter.ai/
Sign in and navigate to Keys
Create a new key and copy it"
            }
            ProviderType::Gemini => {
                "\
Go to https://aistudio.google.com/apikey
Create a new API key
Copy the key"
            }
            ProviderType::OpenAiCompatible => {
                "\
Get your API key from your provider
You will also need to set base_url
in config.toml after setup"
            }
            ProviderType::KimiCode => {
                "\
Go to https://kimi.moonshot.cn/
Sign in and navigate to API settings
Create a new key and copy it"
            }
        }
    }

    pub fn default_model_for_provider(provider: ProviderType) -> (&'static str, &'static str) {
        match provider {
            ProviderType::Anthropic => ("claude", "claude-sonnet-4-20250514"),
            ProviderType::OpenRouter => ("openrouter", "anthropic/claude-sonnet-4"),
            ProviderType::Gemini => ("gemini", "gemini-2.5-flash"),
            ProviderType::OpenAiCompatible => ("openai", "gpt-4o"),
            ProviderType::KimiCode => ("kimi", "kimi-latest"),
        }
    }

    pub fn advance(&mut self) {
        self.input_buffer.clear();
        self.selection_idx = 0;
        self.step = match &self.step {
            OnboardingStep::Welcome => OnboardingStep::ChooseProvider,
            OnboardingStep::ChooseProvider => OnboardingStep::EnterApiKey,
            OnboardingStep::EnterApiKey => OnboardingStep::ConfigureModel,
            OnboardingStep::ConfigureModel => OnboardingStep::EmbeddingsChoice,
            OnboardingStep::EmbeddingsChoice => {
                if self.embedding_provider == EmbeddingChoice::OpenAi {
                    OnboardingStep::EmbeddingsApiKey
                } else {
                    OnboardingStep::DiscordChoice
                }
            }
            OnboardingStep::EmbeddingsApiKey => OnboardingStep::DiscordChoice,
            OnboardingStep::DiscordChoice => OnboardingStep::Summary,
            OnboardingStep::Summary => OnboardingStep::Summary, // terminal
        };
    }

    pub fn go_back(&mut self) {
        self.input_buffer.clear();
        self.selection_idx = 0;
        self.step = match &self.step {
            OnboardingStep::Welcome => OnboardingStep::Welcome,
            OnboardingStep::ChooseProvider => OnboardingStep::Welcome,
            OnboardingStep::EnterApiKey => OnboardingStep::ChooseProvider,
            OnboardingStep::ConfigureModel => OnboardingStep::EnterApiKey,
            OnboardingStep::EmbeddingsChoice => OnboardingStep::ConfigureModel,
            OnboardingStep::EmbeddingsApiKey => OnboardingStep::EmbeddingsChoice,
            OnboardingStep::DiscordChoice => {
                if self.embedding_provider == EmbeddingChoice::OpenAi {
                    OnboardingStep::EmbeddingsApiKey
                } else {
                    OnboardingStep::EmbeddingsChoice
                }
            }
            OnboardingStep::Summary => OnboardingStep::DiscordChoice,
        };
    }

    /// Apply all onboarding choices to the settings and write the .env file.
    /// Returns a status message.
    pub fn apply(&self, settings: &mut t_koma_core::Settings) -> Result<String, String> {
        let Some(provider) = self.provider else {
            return Err("No provider selected".to_string());
        };

        let alias = if self.model_alias.is_empty() {
            Self::default_model_for_provider(provider).0.to_string()
        } else {
            self.model_alias.clone()
        };

        let model_id = if self.model_id.is_empty() {
            Self::default_model_for_provider(provider).1.to_string()
        } else {
            self.model_id.clone()
        };

        settings.models.insert(
            alias.clone(),
            ModelConfig {
                provider,
                model: model_id,
                base_url: None,
                api_key_env: None,
                routing: None,
                context_window: None,
                headers: None,
                retry_on_empty: None,
            },
        );

        settings.default_model = ModelAliases::single(&alias);

        // Embeddings config
        match self.embedding_provider {
            EmbeddingChoice::Ollama => {
                settings.tools.knowledge.embedding_provider = Some("ollama".to_string());
            }
            EmbeddingChoice::OpenAi => {
                settings.tools.knowledge.embedding_provider = Some("openai".to_string());
                settings.tools.knowledge.embedding_model =
                    Some("text-embedding-3-small".to_string());
            }
            EmbeddingChoice::Skip => {}
        }

        settings.discord.enabled = self.discord_enabled;

        // Write .env file with API keys
        let config_path = t_koma_core::Settings::config_path().map_err(|e| e.to_string())?;
        let config_dir = config_path.parent().ok_or("Invalid config path")?;
        let env_path = config_dir.join(".env");

        let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
        let mut env_lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();

        // Provider API key
        if let Some(key) = &self.api_key {
            let env_var = Self::env_var_for_provider(provider);
            env_lines.retain(|l| !l.starts_with(&format!("{env_var}=")));
            env_lines.push(format!("{env_var}={key}"));
        }

        // OpenAI embeddings key
        if let Some(key) = &self.openai_embedding_key {
            env_lines.retain(|l| !l.starts_with("OPENAI_API_KEY="));
            env_lines.push(format!("OPENAI_API_KEY={key}"));
        }

        // Write .env
        if !env_lines.is_empty() {
            std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
            std::fs::write(&env_path, env_lines.join("\n") + "\n").map_err(|e| e.to_string())?;
        }

        // Save config
        settings.save().map_err(|e| e.to_string())?;

        Ok(format!("Setup complete! Model '{}' configured.", alias))
    }
}
