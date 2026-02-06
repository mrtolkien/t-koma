use t_koma_knowledge::{EmbeddingClient, KnowledgeSettings};

#[tokio::test]
async fn test_ollama_embedding_live() {
    let settings = KnowledgeSettings::default();
    let client = EmbeddingClient::new(&settings);
    let inputs = vec!["hello world".to_string(), "t-koma knowledge".to_string()];

    let embeddings = client.embed_batch(&inputs).await.expect("embedding request");
    assert_eq!(embeddings.len(), inputs.len());
    let dim = embeddings[0].len();
    assert!(dim > 0);
    assert!(embeddings.iter().all(|vec| vec.len() == dim));
}
