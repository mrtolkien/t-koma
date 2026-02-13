#![cfg(any(feature = "slow-tests", feature = "live-tests"))]

#[cfg(feature = "live-tests")]
use t_koma_core::config::EmbeddingProviderKind;
use t_koma_knowledge::{EmbeddingClient, KnowledgeSettings};

#[cfg(feature = "slow-tests")]
#[tokio::test]
async fn test_ollama_embedding_live() {
    let settings = KnowledgeSettings::default();
    let client = EmbeddingClient::new(&settings);
    let inputs = vec!["hello world".to_string(), "t-koma knowledge".to_string()];

    let embeddings = client
        .embed_batch(&inputs)
        .await
        .expect("embedding request");
    assert_eq!(embeddings.len(), inputs.len());
    let dim = embeddings[0].len();
    assert!(dim > 0);
    assert!(embeddings.iter().all(|vec| vec.len() == dim));
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_openrouter_embedding_live() {
    t_koma_core::load_dotenv();

    if std::env::var("OPENROUTER_API_KEY")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        eprintln!("OPENROUTER_API_KEY not set; skipping OpenRouter embedding live test.");
        return;
    }

    let settings = KnowledgeSettings {
        embedding_provider: EmbeddingProviderKind::OpenRouter,
        embedding_url: "https://openrouter.ai/api/v1".to_string(),
        embedding_model: "openai/text-embedding-3-small".to_string(),
        ..KnowledgeSettings::default()
    };

    let client = EmbeddingClient::new(&settings);
    assert_eq!(client.provider_kind(), EmbeddingProviderKind::OpenRouter);

    let inputs = vec!["hello world".to_string(), "t-koma knowledge".to_string()];
    let embeddings = client
        .embed_batch(&inputs)
        .await
        .expect("OpenRouter embedding request");

    assert_eq!(embeddings.len(), inputs.len());
    let dim = embeddings[0].len();
    assert!(dim > 0, "embedding dimension should be > 0, got {dim}");
    assert!(
        embeddings.iter().all(|vec| vec.len() == dim),
        "all embeddings should have the same dimension"
    );
    eprintln!(
        "OpenRouter embedding OK: {} vectors, dim={}",
        embeddings.len(),
        dim
    );
}
