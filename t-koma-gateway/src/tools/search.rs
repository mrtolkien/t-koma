use grep::regex::RegexMatcher;
use grep::searcher::Searcher;
use grep::searcher::sinks::UTF8;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::{Tool, ToolContext};
use super::context::resolve_local_path;

pub struct SearchTool;

/// Detailed instructions for using the search tool
const SEARCH_PROMPT: &str = r#"## Searching Code

You have access to a `search` tool for finding patterns in files.

**Features:**
- Regex pattern support
- Respects .gitignore (won't search ignored files)
- Case-insensitive by default
- Glob filtering for specific file types

**When to use:**
- Finding function definitions or usages
- Searching for specific strings across the codebase
- Finding where a variable or type is used
- Locating configuration or documentation

**Best practices:**
- Use specific patterns to reduce noise
- Use `glob` to filter by file type (e.g., "*.rs" for Rust files)
- For case-sensitive searches (e.g., searching for TypeNames), set `case_sensitive: true`
- Combine with `read_file` to examine found matches in detail

**Example patterns:**
- `"fn my_function"` - Find function definitions
- `"struct MyStruct"` - Find struct definitions  
- `"impl.*Trait"` - Find trait implementations (regex)
- `"todo!|FIXME"` - Find TODO comments (regex)
"#;

#[async_trait::async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Searches for patterns in files using regex. Respects .gitignore by default. Returns matching lines with file paths and line numbers."
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(SEARCH_PROMPT)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Search pattern (regex supported)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in. Defaults to current working directory."
                },
                "glob": {
                    "type": "string",
                    "description": "File glob pattern to filter results (e.g., '*.rs', '*.py'). Optional."
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Whether search is case sensitive. Defaults to false.",
                    "default": false
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| "Missing or invalid 'pattern' argument".to_string())?;

        let path = args["path"].as_str().unwrap_or(".");
        let resolved_path = resolve_local_path(context, path)?;

        let glob_pattern = args["glob"].as_str();
        let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);

        // Build regex pattern
        let regex_pattern = if case_sensitive {
            pattern.to_string()
        } else {
            // For case-insensitive search, we use regex flag
            format!("(?i){}", pattern)
        };

        // Create matcher
        let matcher = RegexMatcher::new(&regex_pattern)
            .map_err(|e| format!("Invalid regex pattern '{}': {}", pattern, e))?;

        // Build ignore walker (respects .gitignore)
        let mut walker_builder = ignore::WalkBuilder::new(&resolved_path);
        walker_builder.hidden(false);
        walker_builder.git_ignore(true);
        walker_builder.git_global(true);
        walker_builder.git_exclude(true);
        walker_builder.require_git(false); // Respect gitignore even without git repo

        // Apply glob filter if specified
        if let Some(glob) = glob_pattern {
            let filter = ignore::overrides::OverrideBuilder::new(&resolved_path)
                .add(glob)
                .map_err(|e| format!("Invalid glob pattern '{}': {}", glob, e))?
                .build()
                .map_err(|e| format!("Failed to build glob filter: {}", e))?;
            walker_builder.overrides(filter);
        }

        let walker = walker_builder.build();

        // Collect results
        let results: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // Run search in blocking thread since grep is synchronous
        let results_clone = results.clone();
        let path_owned = resolved_path.to_string_lossy().to_string();

        tokio::task::spawn_blocking(move || {
            let mut searcher = Searcher::new();

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let file_path = entry.path();
                if !file_path.is_file() {
                    continue;
                }

                // Skip binary files
                if is_binary_file(file_path) {
                    continue;
                }

                let results_inner = results_clone.clone();
                let matcher_clone = matcher.clone();

                // Search the file
                let _ = searcher.search_path(
                    &matcher_clone,
                    file_path,
                    UTF8(move |line_num, line| {
                        let line_clean = line.trim_end();
                        let result_line =
                            format!("{}:{}: {}", file_path.display(), line_num, line_clean);
                        if let Ok(mut guard) = results_inner.lock() {
                            guard.push(result_line);
                        }
                        Ok(true)
                    }),
                );
            }

            Ok::<(), String>(())
        })
        .await
        .map_err(|e| format!("Search task failed: {}", e))??;

        // Extract results
        let final_results = results
            .lock()
            .map_err(|e| format!("Failed to lock results: {}", e))?;

        if final_results.is_empty() {
            return Ok(format!(
                "No matches found for pattern '{}' in '{}'",
                pattern, path_owned
            ));
        }

        // Format output
        let mut output = String::new();
        output.push_str(&format!(
            "Found {} match(es) for pattern '{}':\n\n",
            final_results.len(),
            pattern
        ));

        for result in final_results.iter() {
            output.push_str(result);
            output.push('\n');
        }

        Ok(output)
    }
}

/// Check if a file is likely binary based on extension
fn is_binary_file(path: &Path) -> bool {
    let binary_extensions = [
        "exe", "dll", "so", "dylib", "bin", "o", "obj", "a", "lib", "png", "jpg", "jpeg", "gif",
        "bmp", "ico", "svg", "mp3", "mp4", "wav", "avi", "mov", "webm", "zip", "tar", "gz", "bz2",
        "xz", "7z", "rar", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "db", "sqlite",
        "sqlite3",
    ];

    if let Some(ext) = path.extension()
        && let Some(ext_str) = ext.to_str()
    {
        return binary_extensions.contains(&ext_str.to_lowercase().as_str());
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_search_simple_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "Hello, World!").unwrap();
        writeln!(file, "This is a test").unwrap();
        writeln!(file, "Hello again").unwrap();

        let tool = SearchTool;
        let args = json!({
            "pattern": "Hello",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Found 2 match(es)"));
        assert!(output.contains("Hello, World!"));
        assert!(output.contains("Hello again"));
    }

    #[tokio::test]
    async fn test_search_regex_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "function test()").unwrap();
        writeln!(file, "function another()").unwrap();
        writeln!(file, "not a function").unwrap();

        let tool = SearchTool;
        let args = json!({
            "pattern": "^function",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Found 2 match(es)"));
    }

    #[tokio::test]
    async fn test_search_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "Hello, World!").unwrap();

        let tool = SearchTool;
        let args = json!({
            "pattern": "nonexistent_pattern_12345",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("No matches found"));
    }

    #[tokio::test]
    async fn test_search_case_sensitive() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "Hello").unwrap();
        writeln!(file, "hello").unwrap();

        let tool = SearchTool;

        // Case insensitive (default) - should find both
        let args = json!({
            "pattern": "hello",
            "path": temp_dir.path().to_str().unwrap(),
            "case_sensitive": false
        });
        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(result.contains("Found 2 match(es)"));

        // Case sensitive - should find only lowercase
        let args = json!({
            "pattern": "hello",
            "path": temp_dir.path().to_str().unwrap(),
            "case_sensitive": true
        });
        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(result.contains("Found 1 match(es)"));
    }

    #[tokio::test]
    async fn test_search_glob_filter() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        let rs_path = temp_dir.path().join("test.rs");
        let mut rs_file = std::fs::File::create(&rs_path).unwrap();
        writeln!(rs_file, "fn main() {{}}").unwrap();

        let txt_path = temp_dir.path().join("test.txt");
        let mut txt_file = std::fs::File::create(&txt_path).unwrap();
        writeln!(txt_file, "fn main() {{}}").unwrap();

        let tool = SearchTool;
        let args = json!({
            "pattern": "fn main",
            "path": temp_dir.path().to_str().unwrap(),
            "glob": "*.rs"
        });

        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(result.contains("test.rs"));
        assert!(!result.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_search_respects_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        // Create .gitignore
        let gitignore_path = temp_dir.path().join(".gitignore");
        std::fs::write(&gitignore_path, "ignored.txt\n").unwrap();

        // Create regular file
        let file_path = temp_dir.path().join("regular.txt");
        std::fs::write(&file_path, "target").unwrap();

        // Create ignored file
        let ignored_path = temp_dir.path().join("ignored.txt");
        std::fs::write(&ignored_path, "target").unwrap();

        let tool = SearchTool;
        let args = json!({
            "pattern": "target",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(result.contains("regular.txt"));
        assert!(!result.contains("ignored.txt"));
    }
}
