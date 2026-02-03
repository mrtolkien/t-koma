//! Live tests for web tools (requires --features live-tests and valid API keys).

#[cfg(feature = "live-tests")]
use std::time::Duration;

#[cfg(feature = "live-tests")]
use t_koma_gateway::web::search::{brave::BraveSearchProvider, SearchProvider, WebSearchQuery};
#[cfg(feature = "live-tests")]
use t_koma_gateway::web::fetch::{http::HttpFetchProvider, FetchProvider, WebFetchRequest};

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_brave_web_search_rust_language() {
    t_koma_core::load_dotenv();
    let api_key = match std::env::var("BRAVE_API_KEY") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("BRAVE_API_KEY not set; skipping live Brave web search test.");
            return;
        }
    };

    let provider = BraveSearchProvider::new(
        api_key,
        Duration::from_secs(20),
        Duration::from_millis(1000),
    )
    .expect("Failed to create BraveSearchProvider");

    let response = provider
        .search(&WebSearchQuery {
            query: "rust programming language".to_string(),
            count: Some(10),
            country: None,
            search_lang: None,
            ui_lang: None,
            freshness: None,
        })
        .await
        .expect("Brave search failed");

    let urls: Vec<String> = response
        .results
        .iter()
        .map(|result| result.url.to_lowercase())
        .collect();

    let has_wikipedia = urls.iter().any(|url| url.contains("wikipedia.org"));
    let has_rust_lang = urls.iter().any(|url| url.contains("rust-lang.org"));

    assert!(
        has_wikipedia,
        "Expected wikipedia.org result for Rust programming language"
    );
    assert!(
        has_rust_lang,
        "Expected rust-lang.org result for Rust programming language"
    );
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_web_fetch_github_profile() {
    t_koma_core::load_dotenv();

    let provider = HttpFetchProvider::new(
        Duration::from_secs(20),
        "text".to_string(),
        12000,
    )
    .expect("Failed to create HttpFetchProvider");

    let response = provider
        .fetch(&WebFetchRequest {
            url: "https://github.com/mrtolkien/".to_string(),
            mode: Some("text".to_string()),
            max_chars: Some(12000),
        })
        .await
        .expect("web_fetch failed");

    assert_eq!(response.status, 200);
    assert!(
        response.content.contains("mrtolkien") || response.content.contains("MRTolkien"),
        "Expected fetched content to include profile name"
    );
}
