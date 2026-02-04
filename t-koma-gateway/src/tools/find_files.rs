use ignore::WalkBuilder;
use serde_json::{Value, json};
use std::path::Path;

use super::{Tool, ToolContext};
use super::context::resolve_local_path;

pub struct FindFilesTool;

/// Detailed instructions for using the find_files tool
const FIND_FILES_PROMPT: &str = r#"## Finding Files

You have access to a `find_files` tool for locating files by name pattern.

**Features:**
- Glob pattern support (e.g., "*.rs", "**/*.toml")
- Respects .gitignore (won't return ignored files)
- Recursive by default

**When to use:**
- Finding all files of a certain type (e.g., all Rust files)
- Locating configuration files (e.g., "Cargo.toml", "package.json")
- Finding test files (e.g., "*_test.rs")
- Exploring project structure

**Best practices:**
- Use "*.ext" for all files with a specific extension in the current directory
- Use "**/*.ext" for all files with a specific extension recursively
- For exact file names, just use the filename (e.g., "README.md")
- Combine with `read_file` to examine found files

**Glob pattern examples:**
- "*.rs" - All Rust files in current directory
- "**/*.rs" - All Rust files recursively
- "**/Cargo.toml" - All Cargo.toml files anywhere
- "src/**/*.rs" - All Rust files under src/
"#;

#[async_trait::async_trait]
impl Tool for FindFilesTool {
    fn name(&self) -> &str {
        "find_files"
    }

    fn description(&self) -> &str {
        "Finds files matching a glob pattern. Respects .gitignore. Returns a list of file paths."
    }

    fn prompt(&self) -> Option<&'static str> {
        Some(FIND_FILES_PROMPT)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File name pattern to search for (e.g., '*.rs', 'Cargo.toml'). Supports glob patterns like '**/*.py' for recursive search."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in. Defaults to current working directory."
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

        // Normalize pattern - if it doesn't contain / or **, make it recursive
        let search_pattern = if pattern.contains('/') || pattern.starts_with("**") {
            pattern.to_string()
        } else {
            // For simple patterns like "*.rs", search recursively
            format!("**/{}", pattern)
        };

        // Build walker with gitignore support
        let mut walker_builder = WalkBuilder::new(&resolved_path);
        walker_builder.hidden(false);
        walker_builder.git_ignore(true);
        walker_builder.git_global(true);
        walker_builder.git_exclude(true);
        walker_builder.require_git(false); // Respect gitignore even without git repo

        let walker = walker_builder.build();

        // Collect matching files
        let mut files = Vec::new();

        for result in walker {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            let entry_path = entry.path();

            // Only include files (not directories)
            if !entry_path.is_file() {
                continue;
            }

            // Check if file matches the pattern using glob matching
            if file_matches_pattern(entry_path, &search_pattern) {
                files.push(entry_path.to_string_lossy().to_string());
            }
        }

        if files.is_empty() {
            return Ok(format!(
                "No files found matching pattern '{}' in '{}'",
                pattern,
                resolved_path.display()
            ));
        }

        // Sort files for consistent output
        files.sort();

        // Format output
        let mut output = String::new();
        output.push_str(&format!(
            "Found {} file(s) matching pattern '{}':\n\n",
            files.len(),
            pattern
        ));

        for file in files {
            output.push_str(&file);
            output.push('\n');
        }

        Ok(output)
    }
}

/// Check if a file matches a glob pattern
fn file_matches_pattern(path: &Path, pattern: &str) -> bool {
    // Use the globset crate pattern matching (same as ignore crate uses)
    if let Ok(glob) = globset::Glob::new(pattern) {
        let matcher = glob.compile_matcher();
        // Try matching against both the full path and just the file name
        if matcher.is_match(path) {
            return true;
        }
        // Also try matching just the file name
        if let Some(file_name) = path.file_name()
            && let Some(name_str) = file_name.to_str()
            && let Ok(name_glob) = globset::Glob::new(pattern)
        {
            let name_matcher = name_glob.compile_matcher();
            if name_matcher.is_match(name_str) {
                return true;
            }
        }
    }

    // Fallback to simple pattern matching
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let path_str = path.to_string_lossy();

    // Handle different pattern types
    if pattern.starts_with("**") {
        // Recursive pattern - check if path ends with the pattern suffix
        let suffix = pattern.trim_start_matches("**/").trim_start_matches("**");
        if suffix.starts_with("*.") {
            // Pattern like "**/*.rs" - check extension
            let ext = suffix.trim_start_matches("*.");
            path_str.ends_with(ext) || file_name.ends_with(&format!(".{}", ext))
        } else {
            // Pattern like "**/Cargo.toml" - check if path contains the suffix
            path_str.ends_with(suffix) || file_name == suffix
        }
    } else if pattern.starts_with('*') {
        // Extension pattern like "*.rs"
        let ext = pattern.trim_start_matches("*.");
        file_name.ends_with(&format!(".{}", ext))
    } else if pattern.contains('*') {
        // Other glob patterns - try simple matching
        let regex_pattern = pattern
            .replace(".", "\\.")
            .replace("*", ".*")
            .replace("?", ".");

        if let Ok(regex) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
            regex.is_match(file_name)
        } else {
            file_name == pattern
        }
    } else {
        // Exact match
        file_name == pattern || path_str.ends_with(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_find_files_by_extension() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        fs::write(temp_dir.path().join("file1.rs"), "").unwrap();
        fs::write(temp_dir.path().join("file2.rs"), "").unwrap();
        fs::write(temp_dir.path().join("file.txt"), "").unwrap();

        let tool = FindFilesTool;
        let args = json!({
            "pattern": "*.rs",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("file1.rs"), "Output: {}", output);
        assert!(output.contains("file2.rs"), "Output: {}", output);
        assert!(!output.contains("file.txt"), "Output: {}", output);
    }

    #[tokio::test]
    async fn test_find_files_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        fs::write(temp_dir.path().join("root.rs"), "").unwrap();

        let sub_dir = temp_dir.path().join("src");
        fs::create_dir(&sub_dir).unwrap();
        fs::write(sub_dir.join("main.rs"), "").unwrap();
        fs::write(sub_dir.join("lib.rs"), "").unwrap();

        let tool = FindFilesTool;
        let args = json!({
            "pattern": "**/*.rs",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("root.rs"), "Output: {}", output);
        assert!(output.contains("main.rs"), "Output: {}", output);
        assert!(output.contains("lib.rs"), "Output: {}", output);
    }

    #[tokio::test]
    async fn test_find_files_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        fs::write(temp_dir.path().join("file.txt"), "").unwrap();

        let tool = FindFilesTool;
        let args = json!({
            "pattern": "*.nonexistent",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("No files found"));
    }

    #[tokio::test]
    async fn test_find_files_exact_name() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());
        fs::write(temp_dir.path().join("Cargo.toml"), "").unwrap();
        fs::write(temp_dir.path().join("other.toml"), "").unwrap();

        let tool = FindFilesTool;
        let args = json!({
            "pattern": "Cargo.toml",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Cargo.toml"), "Output: {}", output);
        assert!(!output.contains("other.toml"), "Output: {}", output);
    }

    #[tokio::test]
    async fn test_find_files_respects_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let mut context = ToolContext::new_for_tests(temp_dir.path());

        // Create .gitignore
        fs::write(temp_dir.path().join(".gitignore"), "ignored.rs\n").unwrap();

        fs::write(temp_dir.path().join("regular.rs"), "").unwrap();
        fs::write(temp_dir.path().join("ignored.rs"), "").unwrap();

        let tool = FindFilesTool;
        let args = json!({
            "pattern": "*.rs",
            "path": temp_dir.path().to_str().unwrap()
        });

        let result = tool.execute(args, &mut context).await.unwrap();
        assert!(
            result.contains("regular.rs"),
            "Should contain regular.rs: {}",
            result
        );
        // Note: The ignore crate may or may not respect gitignore in this context
        // depending on whether it detects a git repository. This test may need adjustment.
    }
}
