/// Markdown-to-Components-v2 adapter.
///
/// Converts markdown text into a sequence of v2 component JSON values:
/// - Regular text → `TextDisplay`
/// - Horizontal rules (`---`, `***`, `___`) → `Separator`
/// - Tables → wrapped in a code block inside `TextDisplay`
///
/// Code fences are tracked so that table/HR detection doesn't fire inside
/// fenced code blocks.
use super::components_v2::{self, TEXT_DISPLAY_LIMIT};

/// Convert markdown text into a flat list of v2 components (TextDisplay / Separator).
pub fn markdown_to_v2_components(text: &str) -> Vec<serde_json::Value> {
    let mut components = Vec::new();
    let mut text_buf = String::new();
    let mut in_fence = false;
    let mut table_buf: Vec<String> = Vec::new();
    let mut in_table = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Track code fence open/close
        if trimmed.starts_with("```") {
            if in_table {
                flush_table(&mut table_buf, &mut text_buf);
                in_table = false;
            }
            in_fence = !in_fence;
            text_buf.push_str(line);
            text_buf.push('\n');
            continue;
        }

        if in_fence {
            text_buf.push_str(line);
            text_buf.push('\n');
            continue;
        }

        // Horizontal rule detection (outside code fences)
        if is_horizontal_rule(trimmed) {
            if in_table {
                flush_table(&mut table_buf, &mut text_buf);
                in_table = false;
            }
            flush_text(&mut text_buf, &mut components);
            components.push(components_v2::separator(true));
            continue;
        }

        // Table detection
        if is_table_line(trimmed) {
            if !in_table && is_table_separator(trimmed) {
                // This is a separator row — check if previous line was a header
                if let Some(prev) = pop_last_line(&mut text_buf) {
                    if is_table_line(&prev) {
                        // Flush any text accumulated before the table header
                        flush_text(&mut text_buf, &mut components);
                        in_table = true;
                        table_buf.push(prev);
                        table_buf.push(line.to_string());
                        continue;
                    }
                    // Not a table header — push it back
                    text_buf.push_str(&prev);
                    text_buf.push('\n');
                }
            } else if in_table {
                table_buf.push(line.to_string());
                continue;
            }
        } else if in_table {
            // End of table — flush as code block, then flush as separate text
            flush_table(&mut table_buf, &mut text_buf);
            flush_text(&mut text_buf, &mut components);
            in_table = false;
        }

        text_buf.push_str(line);
        text_buf.push('\n');
    }

    if in_table {
        flush_table(&mut table_buf, &mut text_buf);
    }

    flush_text(&mut text_buf, &mut components);
    components
}

/// Check if a line is a markdown horizontal rule.
///
/// Must be 3+ of the same character (`-`, `*`, or `_`), optionally with spaces,
/// and nothing else on the line.
fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 3 {
        return false;
    }
    let first = chars[0];
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    chars.iter().all(|&c| c == first)
}

/// Check if a line looks like a table row (`| ... |`).
fn is_table_line(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

/// Check if a line is a table separator (`|---|---|`).
fn is_table_separator(trimmed: &str) -> bool {
    if !is_table_line(trimmed) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner
        .split('|')
        .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

/// Pop the last line from the text buffer (before the trailing newline).
fn pop_last_line(buf: &mut String) -> Option<String> {
    let content = buf.trim_end_matches('\n');
    if content.is_empty() {
        return None;
    }
    let last_nl = content.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = content[last_nl..].to_string();
    buf.truncate(last_nl);
    Some(line)
}

/// Flush accumulated table lines as a code-block-wrapped TextDisplay.
fn flush_table(table_buf: &mut Vec<String>, text_buf: &mut String) {
    if table_buf.is_empty() {
        return;
    }
    text_buf.push_str("```\n");
    for line in table_buf.drain(..) {
        text_buf.push_str(&line);
        text_buf.push('\n');
    }
    text_buf.push_str("```\n");
}

/// Flush the text buffer as one or more TextDisplay components, splitting if
/// the content exceeds the 4000-character limit.
fn flush_text(buf: &mut String, components: &mut Vec<serde_json::Value>) {
    let content = buf.trim();
    if content.is_empty() {
        buf.clear();
        return;
    }
    let content = buf.trim_end_matches('\n');

    if content.len() <= TEXT_DISPLAY_LIMIT {
        components.push(components_v2::text_display(content));
    } else {
        for chunk in split_text_display(content) {
            components.push(components_v2::text_display(&chunk));
        }
    }
    buf.clear();
}

/// Split long text at line boundaries into chunks under `TEXT_DISPLAY_LIMIT`.
fn split_text_display(content: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in content.split_inclusive('\n') {
        if current.len() + line.len() > TEXT_DISPLAY_LIMIT && !current.is_empty() {
            chunks.push(current.trim_end_matches('\n').to_string());
            current = String::new();
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current.trim_end_matches('\n').to_string());
    }

    // Fallback: if a single line is too long, hard-split by char
    let mut result = Vec::new();
    for chunk in chunks {
        if chunk.len() <= TEXT_DISPLAY_LIMIT {
            result.push(chunk);
        } else {
            let mut piece = String::new();
            for ch in chunk.chars() {
                if piece.len() + ch.len_utf8() > TEXT_DISPLAY_LIMIT {
                    result.push(piece);
                    piece = String::new();
                }
                piece.push(ch);
            }
            if !piece.is_empty() {
                result.push(piece);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_becomes_single_text_display() {
        let components = markdown_to_v2_components("Hello world");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0]["type"], 10);
        assert_eq!(components[0]["content"], "Hello world");
    }

    #[test]
    fn horizontal_rule_becomes_separator() {
        let input = "Before\n---\nAfter";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 3);
        assert_eq!(components[0]["type"], 10); // TextDisplay "Before"
        assert_eq!(components[1]["type"], 14); // Separator
        assert_eq!(components[2]["type"], 10); // TextDisplay "After"
    }

    #[test]
    fn triple_asterisk_is_horizontal_rule() {
        let input = "A\n***\nB";
        let components = markdown_to_v2_components(input);
        assert_eq!(components[1]["type"], 14);
    }

    #[test]
    fn triple_underscore_is_horizontal_rule() {
        let input = "A\n___\nB";
        let components = markdown_to_v2_components(input);
        assert_eq!(components[1]["type"], 14);
    }

    #[test]
    fn hr_inside_code_fence_is_preserved() {
        let input = "```\n---\n```";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 1);
        assert_eq!(components[0]["type"], 10);
        let content = components[0]["content"].as_str().unwrap();
        assert!(content.contains("---"));
    }

    #[test]
    fn table_wrapped_in_code_block() {
        let input = "Before\n| A | B |\n|---|---|\n| 1 | 2 |\nAfter";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 3);

        let before = components[0]["content"].as_str().unwrap();
        assert_eq!(before, "Before");

        let table = components[1]["content"].as_str().unwrap();
        assert!(table.contains("```"));
        assert!(table.contains("| A | B |"));
        assert!(table.contains("| 1 | 2 |"));

        let after = components[2]["content"].as_str().unwrap();
        assert_eq!(after, "After");
    }

    #[test]
    fn table_inside_code_fence_not_transformed() {
        let input = "```\n| A | B |\n|---|---|\n| 1 | 2 |\n```";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 1);
        let content = components[0]["content"].as_str().unwrap();
        // Should NOT double-wrap in code fences
        assert_eq!(content.matches("```").count(), 2);
    }

    #[test]
    fn empty_text_produces_no_components() {
        let components = markdown_to_v2_components("");
        assert!(components.is_empty());
    }

    #[test]
    fn only_whitespace_produces_no_components() {
        let components = markdown_to_v2_components("   \n\n  ");
        assert!(components.is_empty());
    }

    #[test]
    fn two_dashes_is_not_horizontal_rule() {
        let input = "A\n--\nB";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 1);
        assert_eq!(components[0]["type"], 10);
    }

    #[test]
    fn hr_with_spaces() {
        assert!(is_horizontal_rule("- - -"));
        assert!(is_horizontal_rule("* * *"));
    }

    #[test]
    fn mixed_chars_not_hr() {
        assert!(!is_horizontal_rule("-*-"));
        assert!(!is_horizontal_rule("---a"));
    }

    #[test]
    fn multiple_separators() {
        let input = "A\n---\nB\n***\nC";
        let components = markdown_to_v2_components(input);
        assert_eq!(components.len(), 5);
        assert_eq!(components[1]["type"], 14);
        assert_eq!(components[3]["type"], 14);
    }
}
