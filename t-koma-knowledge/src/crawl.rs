//! Web crawling for documentation sites.
//!
//! BFS crawl within a single domain, converting HTML to markdown.
//! Used by the `crawl` source type in `reference_import`.

use std::collections::{HashSet, VecDeque};

use tracing::{debug, warn};
use url::Url;

use crate::errors::{KnowledgeError, KnowledgeResult};

/// Configuration for a crawl operation.
pub struct CrawlConfig {
    pub seed_url: Url,
    /// Max link-hop depth from the seed (default 1, max 3).
    pub max_depth: u8,
    /// Max pages to fetch (default 50, max 200).
    pub max_pages: usize,
}

/// A single crawled page with its content converted to markdown.
pub struct CrawledPage {
    pub url: String,
    pub content: String,
    pub filename: String,
}

/// Crawl a domain starting from the seed URL using BFS.
///
/// Only follows links on the same host. Respects depth and page limits.
pub async fn crawl_domain(config: &CrawlConfig) -> KnowledgeResult<Vec<CrawledPage>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| KnowledgeError::SourceFetch(format!("reqwest client: {}", e)))?;

    let seed_host = config
        .seed_url
        .host_str()
        .ok_or_else(|| KnowledgeError::SourceFetch("seed URL has no host".to_string()))?
        .to_string();

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, u8)> = VecDeque::new();
    let mut pages: Vec<CrawledPage> = Vec::new();

    let seed = normalize_url(&config.seed_url);
    visited.insert(seed.clone());
    queue.push_back((seed, 0));

    while let Some((url, depth)) = queue.pop_front() {
        if pages.len() >= config.max_pages {
            break;
        }

        debug!(url = %url, depth, "crawling page");

        let html = match fetch_page(&client, &url).await {
            Ok(html) => html,
            Err(e) => {
                warn!(url = %url, error = %e, "skipping page");
                continue;
            }
        };

        // Extract links before converting to markdown
        if depth < config.max_depth {
            let links = extract_links(&html, &url, &seed_host);
            for link in links {
                if !visited.contains(&link) && visited.len() < config.max_pages * 2 {
                    visited.insert(link.clone());
                    queue.push_back((link, depth + 1));
                }
            }
        }

        let markdown = html2text::from_read(html.as_bytes(), 80);
        let filename = super::sources::url_to_filename(&url);

        pages.push(CrawledPage {
            url,
            content: markdown,
            filename,
        });
    }

    if pages.is_empty() {
        return Err(KnowledgeError::SourceFetch(
            "crawl produced no pages".to_string(),
        ));
    }

    Ok(pages)
}

async fn fetch_page(client: &reqwest::Client, url: &str) -> KnowledgeResult<String> {
    let response = client
        .get(url)
        .header("User-Agent", "t-koma-knowledge/0.1")
        .send()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("HTTP fetch {}: {}", url, e)))?;

    if !response.status().is_success() {
        return Err(KnowledgeError::SourceFetch(format!(
            "HTTP {} for {}",
            response.status(),
            url
        )));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
        return Err(KnowledgeError::SourceFetch(format!(
            "non-HTML content-type '{}' for {}",
            content_type, url
        )));
    }

    response
        .text()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("read body: {}", e)))
}

/// Extract same-host links from HTML using CSS selectors.
fn extract_links(html: &str, base_url: &str, allowed_host: &str) -> Vec<String> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("a[href]").expect("valid selector");

    let base = Url::parse(base_url).ok();

    document
        .select(&selector)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            resolve_link(href, base.as_ref(), allowed_host)
        })
        .collect()
}

/// Resolve a link relative to a base URL, filtering to same-host only.
fn resolve_link(href: &str, base: Option<&Url>, allowed_host: &str) -> Option<String> {
    // Skip fragments, javascript:, mailto:, etc.
    if href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:") {
        return None;
    }

    let resolved = if let Ok(abs) = Url::parse(href) {
        abs
    } else {
        base?.join(href).ok()?
    };

    // Same-host check
    if resolved.host_str() != Some(allowed_host) {
        return None;
    }

    // Only HTTP(S)
    if resolved.scheme() != "http" && resolved.scheme() != "https" {
        return None;
    }

    Some(normalize_url(&resolved))
}

/// Normalize URL: strip fragment, ensure consistent trailing slash behavior.
fn normalize_url(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    normalized.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_links_same_host() {
        let html = r##"
        <html><body>
          <a href="/guide/state">State</a>
          <a href="https://example.com/other">Other</a>
          <a href="/guide/routing">Routing</a>
          <a href="#section">Fragment</a>
          <a href="javascript:void(0)">JS</a>
        </body></html>"##;

        let links = extract_links(html, "https://docs.example.com/guide/", "docs.example.com");
        assert_eq!(links.len(), 2);
        assert!(links[0].contains("/guide/state"));
        assert!(links[1].contains("/guide/routing"));
    }

    #[test]
    fn test_normalize_url_strips_fragment() {
        let url = Url::parse("https://example.com/page#section").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn test_resolve_link_rejects_different_host() {
        let base = Url::parse("https://docs.example.com/").unwrap();
        assert!(resolve_link("https://other.com/page", Some(&base), "docs.example.com").is_none());
    }

    #[test]
    fn test_resolve_link_resolves_relative() {
        let base = Url::parse("https://docs.example.com/guide/").unwrap();
        let result = resolve_link("state", Some(&base), "docs.example.com");
        assert_eq!(
            result,
            Some("https://docs.example.com/guide/state".to_string())
        );
    }
}
