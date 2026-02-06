//! Source fetching for reference topics.
//!
//! Handles cloning git repos (via `gh` CLI for GitHub, `git` for others)
//! and fetching web pages with HTML-to-markdown conversion.

use std::path::Path;

use tracing::{info, warn};

use crate::errors::{KnowledgeError, KnowledgeResult};
use crate::models::TopicSourceInput;
use crate::parser::TopicSource;

/// Metadata about a git repository, gathered before the actual clone.
#[derive(Debug, Clone)]
pub struct GitRepoMetadata {
    pub full_name: String,
    pub description: String,
    pub size_kb: u64,
    pub language: String,
    pub default_branch: String,
}

impl GitRepoMetadata {
    /// Human-readable summary for the approval message.
    pub fn summary(&self) -> String {
        let size_mb = self.size_kb as f64 / 1024.0;
        format!(
            "{}: ~{:.0} MB, {}",
            self.full_name, size_mb, self.language
        )
    }
}

/// Result of fetching a single source.
#[derive(Debug)]
pub struct FetchedSource {
    /// Updated source descriptor with commit SHA (for git).
    pub source: TopicSource,
    /// Files copied to the topic directory (relative paths within topic dir).
    pub files: Vec<String>,
}

// ── Metadata queries (Phase 1 — lightweight) ────────────────────────

/// Query GitHub repo metadata via `gh api`.
pub async fn query_github_metadata(owner_repo: &str) -> KnowledgeResult<GitRepoMetadata> {
    let output = tokio::process::Command::new("gh")
        .args(["api", &format!("repos/{}", owner_repo)])
        .output()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("gh api failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KnowledgeError::SourceFetch(format!(
            "gh api repos/{} failed: {}",
            owner_repo, stderr
        )));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| {
            KnowledgeError::SourceFetch(format!("failed to parse gh api response: {}", e))
        })?;

    Ok(GitRepoMetadata {
        full_name: json["full_name"]
            .as_str()
            .unwrap_or(owner_repo)
            .to_string(),
        description: json["description"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        size_kb: json["size"].as_u64().unwrap_or(0),
        language: json["language"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string(),
        default_branch: json["default_branch"]
            .as_str()
            .unwrap_or("main")
            .to_string(),
    })
}

/// Build a human-readable summary of all sources for the approval message.
pub async fn build_approval_summary(sources: &[TopicSourceInput]) -> String {
    let mut parts = Vec::new();

    for source in sources {
        match source.source_type.as_str() {
            "git" => {
                if let Some(owner_repo) = extract_github_owner_repo(&source.url) {
                    match query_github_metadata(&owner_repo).await {
                        Ok(meta) => {
                            let summary = if source.paths.is_some() {
                                format!("{} (filtered paths)", meta.summary())
                            } else {
                                meta.summary()
                            };
                            parts.push(summary);
                        }
                        Err(_) => {
                            parts.push(format!("{} (metadata unavailable)", source.url));
                        }
                    }
                } else {
                    parts.push(format!("{} (non-GitHub git)", source.url));
                }
            }
            "web" => {
                parts.push(format!("web: {}", source.url));
            }
            other => {
                parts.push(format!("unknown source type: {}", other));
            }
        }
    }

    parts.join(" + ")
}

// ── Source fetching (Phase 2 — heavy work) ──────────────────────────

/// Fetch a git source into the topic directory.
///
/// Uses `gh repo clone` for GitHub repos, `git clone` for others.
/// Returns the list of files copied and the commit SHA.
pub async fn fetch_git_source(
    source: &TopicSourceInput,
    topic_dir: &Path,
) -> KnowledgeResult<FetchedSource> {
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| KnowledgeError::SourceFetch(format!("tempdir: {}", e)))?;
    let clone_dir = tmp_dir.path().join("repo");

    let is_github = extract_github_owner_repo(&source.url).is_some();

    // Clone
    if is_github {
        let owner_repo =
            extract_github_owner_repo(&source.url).expect("already checked");
        clone_with_gh(&owner_repo, &clone_dir, source.ref_name.as_deref()).await?;
    } else {
        clone_with_git(&source.url, &clone_dir, source.ref_name.as_deref()).await?;
    }

    // Sparse checkout if paths specified
    if let Some(paths) = &source.paths {
        sparse_checkout(&clone_dir, paths).await?;
    }

    // Get commit SHA
    let commit = get_head_commit(&clone_dir).await?;

    // Copy files to topic dir
    let files = copy_repo_files(&clone_dir, topic_dir, source.paths.as_deref()).await?;

    info!(
        "Fetched git source {}: {} files, commit {}",
        source.url,
        files.len(),
        &commit[..8.min(commit.len())]
    );

    Ok(FetchedSource {
        source: TopicSource {
            source_type: "git".to_string(),
            url: source.url.clone(),
            ref_name: source.ref_name.clone(),
            commit: Some(commit),
            paths: source.paths.clone(),
        },
        files,
    })
}

/// Fetch a web source and save as markdown.
pub async fn fetch_web_source(
    source: &TopicSourceInput,
    topic_dir: &Path,
) -> KnowledgeResult<FetchedSource> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| KnowledgeError::SourceFetch(format!("reqwest client: {}", e)))?;

    let response = client
        .get(&source.url)
        .header("User-Agent", "t-koma-knowledge/0.1")
        .send()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("HTTP fetch {}: {}", source.url, e)))?;

    if !response.status().is_success() {
        return Err(KnowledgeError::SourceFetch(format!(
            "HTTP {} for {}",
            response.status(),
            source.url
        )));
    }

    let html = response
        .text()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("read body: {}", e)))?;

    let markdown = html2text::from_read(html.as_bytes(), 80);

    let filename = url_to_filename(&source.url);
    let path = topic_dir.join(&filename);
    tokio::fs::write(&path, &markdown)
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("write {}: {}", path.display(), e)))?;

    info!("Fetched web source {}: saved as {}", source.url, filename);

    Ok(FetchedSource {
        source: TopicSource {
            source_type: "web".to_string(),
            url: source.url.clone(),
            ref_name: None,
            commit: None,
            paths: None,
        },
        files: vec![filename],
    })
}

/// Fetch all sources for a topic, collecting results.
///
/// Non-fatal per source: logs warnings and continues. Fails only if ALL
/// sources fail.
pub async fn fetch_all_sources(
    sources: &[TopicSourceInput],
    topic_dir: &Path,
) -> KnowledgeResult<Vec<FetchedSource>> {
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for source in sources {
        let result = match source.source_type.as_str() {
            "git" => fetch_git_source(source, topic_dir).await,
            "web" => fetch_web_source(source, topic_dir).await,
            other => {
                warn!("Unknown source type: {}", other);
                continue;
            }
        };

        match result {
            Ok(fetched) => results.push(fetched),
            Err(e) => {
                warn!("Source fetch failed for {}: {}", source.url, e);
                errors.push(format!("{}: {}", source.url, e));
            }
        }
    }

    if results.is_empty() {
        return Err(KnowledgeError::SourceFetch(format!(
            "all sources failed: {}",
            errors.join("; ")
        )));
    }

    Ok(results)
}

// ── Internal helpers ────────────────────────────────────────────────

/// Extract `owner/repo` from a GitHub URL.
fn extract_github_owner_repo(url: &str) -> Option<String> {
    // Handle both https://github.com/owner/repo and git@github.com:owner/repo
    if let Ok(parsed) = url::Url::parse(url)
        && parsed.host_str() == Some("github.com")
    {
        let path = parsed.path().trim_start_matches('/');
        let path = path.trim_end_matches(".git");
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
    }
    None
}

/// Clone a GitHub repo using `gh repo clone`.
async fn clone_with_gh(
    owner_repo: &str,
    dest: &Path,
    ref_name: Option<&str>,
) -> KnowledgeResult<()> {
    let mut args = vec![
        "repo".to_string(),
        "clone".to_string(),
        owner_repo.to_string(),
        dest.display().to_string(),
        "--".to_string(),
        "--depth".to_string(),
        "1".to_string(),
    ];
    if let Some(branch) = ref_name {
        args.push("--branch".to_string());
        args.push(branch.to_string());
    }

    let output = tokio::process::Command::new("gh")
        .args(&args)
        .output()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("gh repo clone: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KnowledgeError::SourceFetch(format!(
            "gh repo clone {} failed: {}",
            owner_repo, stderr
        )));
    }

    Ok(())
}

/// Clone a non-GitHub git repo.
async fn clone_with_git(
    url: &str,
    dest: &Path,
    ref_name: Option<&str>,
) -> KnowledgeResult<()> {
    let mut args = vec![
        "clone",
        "--depth",
        "1",
        "--filter=blob:none",
    ];
    if let Some(branch) = ref_name {
        args.push("--branch");
        args.push(branch);
    }
    let url_str = url;
    args.push(url_str);
    let dest_str = dest.to_str().unwrap_or("");
    args.push(dest_str);

    let output = tokio::process::Command::new("git")
        .args(&args)
        .output()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("git clone: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KnowledgeError::SourceFetch(format!(
            "git clone {} failed: {}",
            url, stderr
        )));
    }

    Ok(())
}

/// Set up sparse checkout for specific paths.
async fn sparse_checkout(repo_dir: &Path, paths: &[String]) -> KnowledgeResult<()> {
    async fn run_git(dir: &Path, args: &[&str]) -> KnowledgeResult<()> {
        let output = tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .await
            .map_err(|e| KnowledgeError::SourceFetch(format!("git sparse-checkout: {}", e)))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(KnowledgeError::SourceFetch(format!(
                "git sparse-checkout failed: {}",
                stderr
            )));
        }
        Ok(())
    }

    run_git(repo_dir, &["sparse-checkout", "init", "--cone"]).await?;

    let mut args: Vec<&str> = vec!["sparse-checkout", "set"];
    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&path_refs);
    run_git(repo_dir, &args).await?;

    Ok(())
}

/// Get the HEAD commit SHA from a repo directory.
async fn get_head_commit(repo_dir: &Path) -> KnowledgeResult<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .await
        .map_err(|e| KnowledgeError::SourceFetch(format!("git rev-parse: {}", e)))?;

    if !output.status.success() {
        return Err(KnowledgeError::SourceFetch(
            "git rev-parse HEAD failed".to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Copy files from the cloned repo into the topic directory.
///
/// Preserves relative paths. Skips `.git/` and common non-content files.
async fn copy_repo_files(
    repo_dir: &Path,
    topic_dir: &Path,
    paths_filter: Option<&[String]>,
) -> KnowledgeResult<Vec<String>> {
    let mut files = Vec::new();

    for entry in walkdir::WalkDir::new(repo_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip .git directory and common non-content
            name != ".git"
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(repo_dir)
            .unwrap_or(entry.path());
        let rel_str = rel_path.to_string_lossy().to_string();

        // If paths filter is set and sparse checkout didn't work perfectly,
        // double-check file matches
        if let Some(filter) = paths_filter
            && !matches_path_filter(&rel_str, filter)
        {
            continue;
        }

        // Skip binary-looking files by extension
        if is_likely_binary(&rel_str) {
            continue;
        }

        let dest = topic_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                KnowledgeError::SourceFetch(format!("mkdir {}: {}", parent.display(), e))
            })?;
        }

        tokio::fs::copy(entry.path(), &dest).await.map_err(|e| {
            KnowledgeError::SourceFetch(format!(
                "copy {} -> {}: {}",
                entry.path().display(),
                dest.display(),
                e
            ))
        })?;

        files.push(rel_str);
    }

    Ok(files)
}

/// Check if a relative path matches any of the path filters.
fn matches_path_filter(rel_path: &str, filters: &[String]) -> bool {
    for filter in filters {
        if filter.ends_with('/') {
            // Directory filter
            if rel_path.starts_with(filter) || rel_path.starts_with(filter.trim_end_matches('/')) {
                return true;
            }
        } else if rel_path == filter || rel_path.starts_with(&format!("{}/", filter)) {
            return true;
        }
    }
    false
}

/// Heuristic: skip files that are likely binary.
fn is_likely_binary(path: &str) -> bool {
    let binary_exts = [
        ".png", ".jpg", ".jpeg", ".gif", ".ico", ".svg", ".woff", ".woff2", ".ttf", ".eot",
        ".mp3", ".mp4", ".wav", ".ogg", ".zip", ".tar", ".gz", ".bz2", ".xz", ".7z",
        ".exe", ".dll", ".so", ".dylib", ".a", ".o", ".obj", ".class", ".jar",
        ".wasm", ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx",
        ".db", ".sqlite", ".sqlite3", ".bin", ".dat",
    ];
    let lower = path.to_lowercase();
    binary_exts.iter().any(|ext| lower.ends_with(ext))
}

/// Convert a URL to a safe filename for saving web pages.
fn url_to_filename(url: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    let path_part = parsed
        .as_ref()
        .map(|u| u.path().trim_matches('/').replace('/', "-"))
        .unwrap_or_default();

    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_default();

    let name = if path_part.is_empty() {
        host
    } else {
        format!("{}-{}", host, path_part)
    };

    // Sanitize
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();

    format!("{}.md", sanitized.trim_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_github_owner_repo() {
        assert_eq!(
            extract_github_owner_repo("https://github.com/DioxusLabs/dioxus"),
            Some("DioxusLabs/dioxus".to_string())
        );
        assert_eq!(
            extract_github_owner_repo("https://github.com/DioxusLabs/dioxus.git"),
            Some("DioxusLabs/dioxus".to_string())
        );
        assert_eq!(
            extract_github_owner_repo("https://gitlab.com/org/repo"),
            None
        );
    }

    #[test]
    fn test_url_to_filename() {
        assert_eq!(
            url_to_filename("https://dioxuslabs.com/learn/0.6/"),
            "dioxuslabs-com-learn-0-6.md"
        );
        assert_eq!(
            url_to_filename("https://example.com"),
            "example-com.md"
        );
    }

    #[test]
    fn test_is_likely_binary() {
        assert!(is_likely_binary("image.png"));
        assert!(is_likely_binary("font.woff2"));
        assert!(!is_likely_binary("main.rs"));
        assert!(!is_likely_binary("README.md"));
    }

    #[test]
    fn test_matches_path_filter() {
        let filters = vec!["docs/".to_string(), "README.md".to_string()];
        assert!(matches_path_filter("docs/guide.md", &filters));
        assert!(matches_path_filter("README.md", &filters));
        assert!(!matches_path_filter("src/main.rs", &filters));
    }
}
