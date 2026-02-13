//! Live tests for web tools (requires --features live-tests and valid API keys).

#[cfg(feature = "live-tests")]
use std::time::Duration;

#[cfg(feature = "live-tests")]
use t_koma_gateway::web::fetch::{FetchProvider, WebFetchRequest, http::HttpFetchProvider};
#[cfg(feature = "live-tests")]
use t_koma_gateway::web::search::{
    SearchProvider, WebSearchQuery, brave::BraveSearchProvider,
    perplexity::PerplexitySearchProvider,
};

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
async fn test_perplexity_web_search_rust_language() {
    t_koma_core::load_dotenv();
    let api_key = match std::env::var("PERPLEXITY_API_KEY") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("PERPLEXITY_API_KEY not set; skipping live Perplexity web search test.");
            return;
        }
    };

    let provider = PerplexitySearchProvider::new(
        api_key,
        Duration::from_secs(20),
        Duration::from_millis(1000),
    )
    .expect("Failed to create PerplexitySearchProvider");

    let response = provider
        .search(&WebSearchQuery {
            query: "rust programming language".to_string(),
            count: Some(5),
            country: None,
            search_lang: None,
            ui_lang: None,
            freshness: None,
        })
        .await
        .expect("Perplexity search failed");

    assert_eq!(response.provider, "perplexity");
    assert!(
        !response.results.is_empty(),
        "Expected at least one result from Perplexity"
    );

    // Check that results have URLs and titles
    for result in &response.results {
        assert!(!result.url.is_empty(), "Result URL should not be empty");
        assert!(!result.title.is_empty(), "Result title should not be empty");
    }

    // At least one result should reference rust-lang.org
    let has_rust_lang = response
        .results
        .iter()
        .any(|r| r.url.to_lowercase().contains("rust-lang.org"));
    assert!(
        has_rust_lang,
        "Expected rust-lang.org in results for 'rust programming language'"
    );
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn test_web_fetch_github_profile() {
    t_koma_core::load_dotenv();

    let provider = HttpFetchProvider::new(Duration::from_secs(20), "text".to_string(), 12000)
        .expect("Failed to create HttpFetchProvider");

    let response = provider
        .fetch(&WebFetchRequest {
            url: "https://github.com/mrtolkien/".to_string(),
            mode: Some("text".to_string()),
            max_chars: Some(12000),
            raw: false,
        })
        .await
        .expect("web_fetch failed");

    assert_eq!(response.status, 200);
    assert!(
        response.content.contains("mrtolkien") || response.content.contains("MRTolkien"),
        "Expected fetched content to include profile name"
    );
}
