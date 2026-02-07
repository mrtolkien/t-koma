use std::path::Path;

use tree_sitter::{Language, Parser};

use crate::errors::{KnowledgeError, KnowledgeResult};

/// Max characters per chunk. ~1500 tokens — well within the 8K context of
/// `qwen3-embedding:8b` and produces better embedding quality than huge chunks.
const MAX_CHUNK_CHARS: usize = 6000;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub title: String,
    pub content: String,
    pub index: usize,
}

pub fn chunk_markdown(input: &str) -> Vec<Chunk> {
    // Short content stays as a single chunk for better embedding quality
    if input.len() < 1500 {
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            return vec![Chunk {
                title: "Intro".to_string(),
                content: trimmed.to_string(),
                index: 0,
            }];
        }
    }

    let mut chunks = Vec::new();
    let mut current_title = String::from("Intro");
    let mut current_lines: Vec<String> = Vec::new();

    for line in input.lines() {
        if let Some(title) = parse_heading(line) {
            if !current_lines.is_empty() {
                let content = current_lines.join("\n").trim().to_string();
                if !content.is_empty() {
                    chunks.push(Chunk {
                        title: current_title.clone(),
                        content,
                        index: chunks.len(),
                    });
                }
            }
            current_title = title;
            current_lines.clear();
        } else {
            current_lines.push(line.to_string());
        }
    }

    if !current_lines.is_empty() {
        let content = current_lines.join("\n").trim().to_string();
        if !content.is_empty() {
            chunks.push(Chunk {
                title: current_title,
                content,
                index: chunks.len(),
            });
        }
    }

    let merged = merge_small_chunks(chunks, 200);
    split_large_chunks(merged)
}

fn parse_heading(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let title = trimmed[hashes..].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn merge_small_chunks(chunks: Vec<Chunk>, min_chars: usize) -> Vec<Chunk> {
    if chunks.len() <= 1 {
        return chunks;
    }

    let mut merged = Vec::new();
    let mut i = 0;
    while i < chunks.len() {
        let mut current = chunks[i].clone();
        while current.content.len() < min_chars && i + 1 < chunks.len() {
            i += 1;
            let next = &chunks[i];
            current.content.push_str("\n\n");
            current.content.push_str(&next.content);
        }
        current.index = merged.len();
        merged.push(current);
        i += 1;
    }

    merged
}

/// Split any chunk exceeding `MAX_CHUNK_CHARS` on paragraph boundaries.
fn split_large_chunks(chunks: Vec<Chunk>) -> Vec<Chunk> {
    let mut result = Vec::new();
    for chunk in chunks {
        if chunk.content.len() <= MAX_CHUNK_CHARS {
            result.push(Chunk {
                index: result.len(),
                ..chunk
            });
            continue;
        }

        let parts = split_on_paragraphs(&chunk.content, MAX_CHUNK_CHARS);
        for (i, part) in parts.into_iter().enumerate() {
            let title = if i == 0 {
                chunk.title.clone()
            } else {
                format!("{} (cont.)", chunk.title)
            };
            result.push(Chunk {
                title,
                content: part,
                index: result.len(),
            });
        }
    }
    result
}

/// Split text into parts of at most `max_chars`, preferring `\n\n` boundaries.
fn split_on_paragraphs(text: &str, max_chars: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut remaining = text;

    while remaining.len() > max_chars {
        // Snap to a char boundary so we never split inside a multi-byte char
        let boundary = remaining.floor_char_boundary(max_chars);
        let search_region = &remaining[..boundary];
        let split_at = search_region
            .rfind("\n\n")
            .or_else(|| search_region.rfind('\n'))
            .unwrap_or(boundary);

        let split_at = split_at.max(1); // avoid zero-length splits
        let (left, right) = remaining.split_at(split_at);
        let trimmed = left.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
        remaining = right.trim_start();
    }

    let trimmed = remaining.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }

    parts
}

pub fn chunk_code(source: &str, path: &Path) -> KnowledgeResult<Vec<Chunk>> {
    let language = language_for_path(path)?;
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| KnowledgeError::UnsupportedLanguage(path.display().to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| KnowledgeError::UnsupportedLanguage(path.display().to_string()))?;

    let mut chunks = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    for node in root.children(&mut cursor) {
        if is_chunk_node(node.kind())
            && let Ok(text) = node.utf8_text(source.as_bytes())
        {
            let title = format!("{}:{}", node.kind(), node.start_position().row + 1);
            chunks.push(Chunk {
                title,
                content: text.to_string(),
                index: chunks.len(),
            });
        }
    }

    if chunks.is_empty() {
        return Ok(vec![Chunk {
            title: "file".to_string(),
            content: source.to_string(),
            index: 0,
        }]);
    }

    Ok(chunks)
}

fn language_for_path(path: &Path) -> KnowledgeResult<Language> {
    let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("");
    let lang = match ext {
        "rs" => tree_sitter_rust::language(),
        "py" => tree_sitter_python::language(),
        "js" | "jsx" => tree_sitter_javascript::language(),
        "ts" | "tsx" => tree_sitter_typescript::language_typescript(),
        "go" => tree_sitter_go::language(),
        _ => {
            return Err(KnowledgeError::UnsupportedLanguage(
                path.display().to_string(),
            ));
        }
    };

    Ok(lang)
}

fn is_chunk_node(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "impl_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "function_definition"
            | "class_definition"
            | "method_definition"
            | "arrow_function"
            | "function_declaration"
            | "method_declaration"
            | "type_declaration"
            | "interface_declaration"
            | "function"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_stays_single_chunk() {
        let input = "# Title\nIntro\n\n## Section\nContent";
        let chunks = chunk_markdown(input);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].title, "Intro");
        assert!(chunks[0].content.contains("Title"));
        assert!(chunks[0].content.contains("Content"));
    }

    #[test]
    fn long_content_splits_by_heading() {
        // Build content over 1500 chars to trigger heading-based splitting
        let filler = "x".repeat(800);
        let input = format!("# Title\n{}\n\n## Section\n{}", filler, filler);
        let chunks = chunk_markdown(&input);
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[0].title, "Title");
        assert_eq!(chunks[1].title, "Section");
    }
}
