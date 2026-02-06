use serde::Deserialize;

use crate::KnowledgeSettings;
use crate::errors::{KnowledgeError, KnowledgeResult};

#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl EmbeddingClient {
    pub fn new(settings: &KnowledgeSettings) -> Self {
        Self {
            base_url: settings.embedding_url.trim_end_matches('/').to_string(),
            model: settings.embedding_model.clone(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn embed_batch(&self, inputs: &[String]) -> KnowledgeResult<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/api/embed", self.base_url);
        let body = EmbedRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };

        let response = self.client.post(&url).json(&body).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(KnowledgeError::Embedding(format!(
                "embedding request failed: {status} {text}"
            )));
        }

        let payload: EmbedResponse = response.json().await?;

        if let Some(embeddings) = payload.embeddings {
            return Ok(embeddings);
        }

        if let Some(embedding) = payload.embedding {
            return Ok(vec![embedding]);
        }

        Err(KnowledgeError::Embedding(
            "embedding response missing vectors".to_string(),
        ))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbedResponse {
    embeddings: Option<Vec<Vec<f32>>>,
    embedding: Option<Vec<f32>>,
}
