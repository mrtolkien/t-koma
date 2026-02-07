use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;

use crate::errors::{KnowledgeError, KnowledgeResult};

#[derive(Debug, Clone, Deserialize)]
pub struct FrontMatter {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub note_type: String,
    pub created_at: DateTime<Utc>,
    pub created_by: CreatedBy,
    pub trust_score: i64,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_validated_by: Option<CreatedBy>,
    pub comments: Option<Vec<CommentEntry>>,
    pub parent: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<Vec<SourceEntry>>,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedBy {
    pub ghost: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct CommentEntry {
    pub ghost: String,
    pub model: String,
    pub at: DateTime<Utc>,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SourceEntry {
    pub path: String,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WikiLink {
    pub target: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedNote {
    pub front: FrontMatter,
    pub body: String,
    pub links: Vec<WikiLink>,
}

pub fn parse_note(raw: &str) -> KnowledgeResult<ParsedNote> {
    let (front_matter, body) = split_front_matter(raw)?;
    let front: FrontMatter = toml::from_str(&front_matter)?;

    if front.id.trim().is_empty() {
        return Err(KnowledgeError::MissingField("id"));
    }
    if front.title.trim().is_empty() {
        return Err(KnowledgeError::MissingField("title"));
    }
    if front.note_type.trim().is_empty() {
        return Err(KnowledgeError::MissingField("type"));
    }

    let links = extract_links(&body);

    Ok(ParsedNote {
        front,
        body,
        links,
    })
}

fn split_front_matter(raw: &str) -> KnowledgeResult<(String, String)> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("+++") {
        return Err(KnowledgeError::InvalidFrontMatter(
            "missing TOML front matter delimiter".to_string(),
        ));
    }

    let mut lines = trimmed.lines();
    let first = lines.next();
    if first.is_none() {
        return Err(KnowledgeError::InvalidFrontMatter(
            "empty front matter".to_string(),
        ));
    }

    let mut front_lines = Vec::new();
    for line in lines.by_ref() {
        if line.trim() == "+++" {
            let front = front_lines.join("\n");
            let body = lines.collect::<Vec<_>>().join("\n");
            return Ok((front, body));
        }
        front_lines.push(line);
    }

    Err(KnowledgeError::InvalidFrontMatter(
        "unterminated front matter".to_string(),
    ))
}

pub(crate) fn extract_links(body: &str) -> Vec<WikiLink> {
    let pattern = Regex::new(r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]").expect("regex");
    pattern
        .captures_iter(body)
        .map(|cap| WikiLink {
            target: cap.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            alias: cap.get(2).map(|m| m.as_str().trim().to_string()),
        })
        .filter(|link| !link.target.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_front_matter_and_links() {
        let raw = r#"+++
id = "note-1"
title = "Test Note"
type = "Concept"
created_at = "2025-01-01T00:00:00Z"
trust_score = 5
[created_by]
ghost = "tester"
model = "test-model"
+++

This is a body with [[Link Target]] and [[Another|Alias]].
"#;

        let parsed = parse_note(raw).expect("parse note");
        assert_eq!(parsed.front.id, "note-1");
        assert_eq!(parsed.front.title, "Test Note");
        assert_eq!(parsed.links.len(), 2);
        assert_eq!(parsed.links[0].target, "Link Target");
        assert_eq!(parsed.links[1].alias.as_deref(), Some("Alias"));
    }
}
