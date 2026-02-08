use sqlx::SqlitePool;

use crate::errors::KnowledgeResult;
use crate::models::{KnowledgeScope, NoteSummary};

pub async fn load_links_out(
    pool: &SqlitePool,
    note_id: &str,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    let rows = if scope.is_shared() {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, String)>(
            r#"SELECT l.target_title, n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope, s.scope
               FROM note_links l
               JOIN notes s ON s.id = l.source_id
               LEFT JOIN notes n ON n.id = l.target_id AND n.owner_ghost IS NULL
               WHERE l.source_id = ? AND s.scope = ? AND s.owner_ghost IS NULL AND l.owner_ghost IS NULL
               LIMIT ?"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, String)>(
            r#"SELECT l.target_title, n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope, s.scope
               FROM note_links l
               JOIN notes s ON s.id = l.source_id
               LEFT JOIN notes n ON n.id = l.target_id AND (n.owner_ghost IS NULL OR n.owner_ghost = ?)
               WHERE l.source_id = ? AND s.scope = ? AND s.owner_ghost = ? AND l.owner_ghost = ?
               LIMIT ?"#,
        )
        .bind(ghost_name)
        .bind(note_id)
        .bind(scope.as_str())
        .bind(ghost_name)
        .bind(ghost_name)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(target_title, id, title, note_type, path, trust_score, scope, source_scope)| {
                let resolved_title = title.unwrap_or_else(|| target_title.clone());
                let resolved_id = id.unwrap_or_else(|| target_title.clone());
                let resolved_type = note_type.unwrap_or_else(|| "Unresolved".to_string());
                let resolved_scope = scope.unwrap_or(source_scope);
                NoteSummary {
                    id: resolved_id,
                    title: resolved_title,
                    entry_type: resolved_type,
                    archetype: None,
                    path: path.map(std::path::PathBuf::from).unwrap_or_default(),
                    scope: resolved_scope.parse().unwrap_or(KnowledgeScope::SharedNote),
                    trust_score: trust_score.unwrap_or(1),
                    score: 0.0,
                    snippet: String::new(),
                }
            },
        )
        .collect())
}

pub async fn load_links_in(
    pool: &SqlitePool,
    note_id: &str,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    let rows = if scope.is_shared() {
        sqlx::query_as::<_, (String, String, String, String, i64, String)>(
            r#"SELECT n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope
               FROM note_links l
               JOIN notes n ON n.id = l.source_id
               WHERE l.target_id = ? AND n.scope = ? AND n.owner_ghost IS NULL AND l.owner_ghost IS NULL
               LIMIT ?"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, String, String, i64, String)>(
            r#"SELECT n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope
               FROM note_links l
               JOIN notes n ON n.id = l.source_id
               WHERE l.target_id = ? AND n.scope = ? AND n.owner_ghost = ? AND l.owner_ghost = ?
               LIMIT ?"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .bind(ghost_name)
        .bind(ghost_name)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, title, entry_type, path, trust_score, scope)| NoteSummary {
                id,
                title,
                entry_type,
                archetype: None,
                path: path.into(),
                scope: scope.parse().unwrap_or(KnowledgeScope::SharedNote),
                trust_score,
                score: 0.0,
                snippet: String::new(),
            },
        )
        .collect())
}

pub async fn load_parent(
    pool: &SqlitePool,
    note_id: &str,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    let rows: Vec<(String, String, String, String, i64, String)> = if scope.is_shared() {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, String)>(
            r#"SELECT child.parent_id, n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope, child.scope
               FROM notes child
               LEFT JOIN notes n ON n.id = child.parent_id AND n.owner_ghost IS NULL
               WHERE child.id = ? AND child.scope = ? AND child.owner_ghost IS NULL
               LIMIT 1"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(parent_id, id, title, note_type, path, trust_score, scope, child_scope)| {
            let resolved_title = title.unwrap_or_else(|| parent_id.clone());
            let resolved_id = id.unwrap_or_else(|| parent_id.clone());
            let resolved_type = note_type.unwrap_or_else(|| "Unresolved".to_string());
            let resolved_scope = scope.unwrap_or(child_scope);
            (
                resolved_id,
                resolved_title,
                resolved_type,
                path.unwrap_or_default(),
                trust_score.unwrap_or(1),
                resolved_scope,
            )
        })
        .collect()
    } else {
        sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<String>, String)>(
            r#"SELECT child.parent_id, n.id, n.title, n.entry_type, n.path, n.trust_score, n.scope, child.scope
               FROM notes child
               LEFT JOIN notes n ON n.id = child.parent_id AND (n.owner_ghost IS NULL OR n.owner_ghost = ?)
               WHERE child.id = ? AND child.scope = ? AND child.owner_ghost = ?
               LIMIT 1"#,
        )
        .bind(ghost_name)
        .bind(note_id)
        .bind(scope.as_str())
        .bind(ghost_name)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(parent_id, id, title, note_type, path, trust_score, scope, child_scope)| {
            let resolved_title = title.unwrap_or_else(|| parent_id.clone());
            let resolved_id = id.unwrap_or_else(|| parent_id.clone());
            let resolved_type = note_type.unwrap_or_else(|| "Unresolved".to_string());
            let resolved_scope = scope.unwrap_or(child_scope);
            (
                resolved_id,
                resolved_title,
                resolved_type,
                path.unwrap_or_default(),
                trust_score.unwrap_or(1),
                resolved_scope,
            )
        })
        .collect()
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, title, entry_type, path, trust_score, scope)| NoteSummary {
                id,
                title,
                entry_type,
                archetype: None,
                path: path.into(),
                scope: scope.parse().unwrap_or(KnowledgeScope::SharedNote),
                trust_score,
                score: 0.0,
                snippet: String::new(),
            },
        )
        .collect())
}

pub async fn load_tags(
    pool: &SqlitePool,
    note_id: &str,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<String>> {
    let rows = if scope.is_shared() {
        sqlx::query_as::<_, (String,)>(
            r#"SELECT t.tag
               FROM note_tags t
               JOIN notes n ON n.id = t.note_id
               WHERE t.note_id = ? AND n.scope = ? AND n.owner_ghost IS NULL"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String,)>(
            r#"SELECT t.tag
               FROM note_tags t
               JOIN notes n ON n.id = t.note_id
               WHERE t.note_id = ? AND n.scope = ? AND n.owner_ghost = ?"#,
        )
        .bind(note_id)
        .bind(scope.as_str())
        .bind(ghost_name)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(|(tag,)| tag).collect())
}

pub async fn expand_links_out(
    pool: &SqlitePool,
    root_id: &str,
    depth: u8,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    expand_links(
        pool,
        vec![root_id.to_string()],
        depth,
        limit,
        true,
        scope,
        ghost_name,
    )
    .await
}

pub async fn expand_links_in(
    pool: &SqlitePool,
    root_id: &str,
    depth: u8,
    limit: usize,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    expand_links(
        pool,
        vec![root_id.to_string()],
        depth,
        limit,
        false,
        scope,
        ghost_name,
    )
    .await
}

pub async fn expand_parents(
    pool: &SqlitePool,
    root_id: &str,
    depth: u8,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    let mut results = Vec::new();
    let mut current = root_id.to_string();
    let mut remaining = depth;
    while remaining > 0 {
        let parents = load_parent(pool, &current, scope, ghost_name).await?;
        if parents.is_empty() {
            break;
        }
        let parent = parents[0].clone();
        current = parent.id.clone();
        results.push(parent);
        remaining -= 1;
    }
    Ok(results)
}

async fn expand_links(
    pool: &SqlitePool,
    roots: Vec<String>,
    depth: u8,
    limit: usize,
    outgoing: bool,
    scope: KnowledgeScope,
    ghost_name: &str,
) -> KnowledgeResult<Vec<NoteSummary>> {
    let mut visited = std::collections::HashSet::new();
    let mut frontier = roots;
    let mut results = Vec::new();
    let mut remaining_depth = depth;

    while remaining_depth > 0 && !frontier.is_empty() && results.len() < limit {
        let mut next_frontier = Vec::new();
        for note_id in frontier {
            if visited.contains(&note_id) {
                continue;
            }
            visited.insert(note_id.clone());
            let links = if outgoing {
                load_links_out(
                    pool,
                    &note_id,
                    limit.saturating_sub(results.len()),
                    scope,
                    ghost_name,
                )
                .await?
            } else {
                load_links_in(
                    pool,
                    &note_id,
                    limit.saturating_sub(results.len()),
                    scope,
                    ghost_name,
                )
                .await?
            };

            for link in links {
                if results.len() >= limit {
                    break;
                }
                if visited.insert(link.id.clone()) {
                    next_frontier.push(link.id.clone());
                    results.push(link);
                }
            }
        }
        frontier = next_frontier;
        remaining_depth -= 1;
    }

    Ok(results)
}
