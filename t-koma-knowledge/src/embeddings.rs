use serde::Deserialize;
use t_koma_core::config::EmbeddingProviderKind;

use crate::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};

#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    provider: EmbeddingProviderKind,
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl EmbeddingClient {
    pub fn new(settings: &KnowledgeSettings) -> Self {
        let base_url = settings.embedding_url.trim_end_matches('/').to_string();

        let api_key = match settings.embedding_provider {
            EmbeddingProviderKind::OpenRouter => std::env::var("OPENROUTER_API_KEY").ok(),
            EmbeddingProviderKind::Ollama => None,
        };

        Self {
            provider: settings.embedding_provider,
            base_url,
            model: settings.embedding_model.clone(),
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn model_id(&self) -> &str {
        &self.model
    }

    pub fn provider_kind(&self) -> EmbeddingProviderKind {
        self.provider
    }

    pub async fn embed_batch(&self, inputs: &[String]) -> KnowledgeResult<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        match self.provider {
            EmbeddingProviderKind::Ollama => self.embed_ollama(inputs).await,
            EmbeddingProviderKind::OpenRouter => self.embed_openrouter(inputs).await,
        }
    }

    async fn embed_ollama(&self, inputs: &[String]) -> KnowledgeResult<Vec<Vec<f32>>> {
        let url = format!("{}/api/embed", self.base_url);
        let body = OllamaEmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };

        let response = self.client.post(&url).json(&body).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(KnowledgeError::Embedding(format!(
                "ollama embedding request failed: {status} {text}"
            )));
        }

        let payload: OllamaEmbedResponse = response.json().await?;

        if let Some(embeddings) = payload.embeddings {
            return Ok(embeddings);
        }
        if let Some(embedding) = payload.embedding {
            return Ok(vec![embedding]);
        }

        Err(KnowledgeError::Embedding(
            "ollama embedding response missing vectors".to_string(),
        ))
    }

    async fn embed_openrouter(&self, inputs: &[String]) -> KnowledgeResult<Vec<Vec<f32>>> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            KnowledgeError::Embedding(
                "OpenRouter embedding provider requires OPENROUTER_API_KEY".to_string(),
            )
        })?;

        let url = format!("{}/embeddings", self.base_url);
        let body = OpenRouterEmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };

        let response = self
            .client
            .post(&url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(KnowledgeError::Embedding(format!(
                "openrouter embedding request failed: {status} {text}"
            )));
        }

        let payload: OpenRouterEmbedResponse = response.json().await?;
        let mut result: Vec<(usize, Vec<f32>)> = payload
            .data
            .into_iter()
            .map(|d| (d.index, d.embedding))
            .collect();
        result.sort_by_key(|(idx, _)| *idx);
        Ok(result.into_iter().map(|(_, v)| v).collect())
    }
}

// ── Ollama wire types ─────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct OllamaEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Option<Vec<Vec<f32>>>,
    embedding: Option<Vec<f32>>,
}

// ── OpenRouter wire types ─────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
struct OpenRouterEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenRouterEmbedResponse {
    data: Vec<OpenRouterEmbedding>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenRouterEmbedding {
    index: usize,
    embedding: Vec<f32>,
}
