//! Session prompt cache for maximizing Anthropic server-side cache hits.
//!
//! Caches the rendered system prompt blocks for a configurable TTL (default
//! 5 minutes). Within that window, every API call for the same session uses
//! byte-identical system blocks, guaranteeing Anthropic cache hits (~90%
//! input cost savings).
//!
//! The cache is backed by both an in-memory map (fast path) and a DB table
//! (survives gateway restarts within the TTL window).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::prompt::render::SystemBlock;
use t_koma_db::{GhostDbPool, PromptCacheEntry, PromptCacheRepository};

/// Default cache TTL in seconds (5 minutes matches Anthropic's cache window).
const DEFAULT_TTL_SECS: i64 = 300;

/// A cached system prompt entry for a session.
#[derive(Debug, Clone)]
struct CacheEntry {
    system_blocks: Vec<SystemBlock>,
    context_hash: String,
    cached_at: i64,
}

impl CacheEntry {
    fn is_valid(&self, now: i64) -> bool {
        now - self.cached_at < DEFAULT_TTL_SECS
    }
}

/// Manages session prompt caching (in-memory + DB-backed).
#[derive(Debug, Clone)]
pub struct PromptCacheManager {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl PromptCacheManager {
    /// Create a new prompt cache manager.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load cached entries from the DB that are still within the TTL.
    ///
    /// Call this on startup to recover caches that survive process restarts.
    pub async fn load_from_db(&self, ghost_db: &GhostDbPool) {
        let cutoff = Utc::now().timestamp() - DEFAULT_TTL_SECS;
        let rows = match PromptCacheRepository::load_recent(ghost_db.pool(), cutoff).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "Failed to load prompt cache from DB");
                return;
            }
        };

        let mut cache = self.cache.write().await;
        for row in rows {
            let blocks: Vec<SystemBlock> = match serde_json::from_str(&row.system_blocks_json) {
                Ok(b) => b,
                Err(e) => {
                    warn!(session_id = row.session_id, error = %e, "Failed to deserialize cached prompt");
                    continue;
                }
            };
            cache.insert(
                row.session_id,
                CacheEntry {
                    system_blocks: blocks,
                    context_hash: row.context_hash,
                    cached_at: row.cached_at,
                },
            );
        }

        debug!(count = cache.len(), "Loaded prompt cache entries from DB");
    }

    /// Get cached system blocks for a session, or build fresh ones.
    ///
    /// If a valid cached entry exists (within TTL), returns the cached blocks.
    /// Otherwise, calls the `build` closure to generate fresh blocks, caches
    /// them, and persists to the DB.
    pub async fn get_or_build<F, Fut>(
        &self,
        session_id: &str,
        ghost_db: &GhostDbPool,
        context_hash: &str,
        build: F,
    ) -> Vec<SystemBlock>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Vec<SystemBlock>>,
    {
        let now = Utc::now().timestamp();

        // Fast path: check in-memory cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(session_id)
                && entry.is_valid(now)
                && entry.context_hash == context_hash
            {
                debug!(session_id, "Prompt cache hit (in-memory)");
                return entry.system_blocks.clone();
            }
        }

        // Cache miss: build fresh blocks
        let blocks = build().await;

        let entry = CacheEntry {
            system_blocks: blocks.clone(),
            context_hash: context_hash.to_string(),
            cached_at: now,
        };

        // Persist to DB (fire-and-forget)
        if let Ok(json) = serde_json::to_string(&entry.system_blocks) {
            let db_entry = PromptCacheEntry {
                session_id: session_id.to_string(),
                system_blocks_json: json,
                context_hash: context_hash.to_string(),
                cached_at: now,
            };
            if let Err(e) = PromptCacheRepository::upsert(ghost_db.pool(), &db_entry).await {
                warn!(session_id, error = %e, "Failed to persist prompt cache");
            }
        }

        // Store in memory
        {
            let mut cache = self.cache.write().await;
            cache.insert(session_id.to_string(), entry);
        }

        debug!(session_id, "Prompt cache miss — built fresh");
        blocks
    }

    /// Invalidate the cache for a session (e.g., when the ghost context changes).
    pub async fn invalidate(&self, session_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(session_id);
    }
}

impl Default for PromptCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a deterministic hash of context variable pairs.
///
/// Used to detect when the ghost context has changed (diary, identity, etc.)
/// and the cached prompt should be invalidated.
pub fn hash_context(vars: &[(&str, &str)]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for (key, value) in vars {
        key.hash(&mut hasher);
        value.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::CacheControl;

    #[test]
    fn test_hash_context_deterministic() {
        let vars = vec![("ghost_identity", "Hello"), ("ghost_diary", "Entry")];
        let h1 = hash_context(&vars);
        let h2 = hash_context(&vars);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_context_changes_with_content() {
        let h1 = hash_context(&[("ghost_identity", "A")]);
        let h2 = hash_context(&[("ghost_identity", "B")]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_cache_entry_validity() {
        let now = Utc::now().timestamp();
        let entry = CacheEntry {
            system_blocks: vec![],
            context_hash: "abc".to_string(),
            cached_at: now,
        };
        assert!(entry.is_valid(now));
        assert!(entry.is_valid(now + 299));
        assert!(!entry.is_valid(now + 300));
        assert!(!entry.is_valid(now + 600));
    }

    #[tokio::test]
    async fn test_cache_miss_then_hit() {
        let cache = PromptCacheManager::new();
        let blocks = vec![
            SystemBlock::new("Instruction 1"),
            SystemBlock::with_cache("Instruction 2", CacheControl::ephemeral()),
        ];
        let blocks_clone = blocks.clone();

        let db = t_koma_db::test_helpers::create_test_ghost_pool("CacheGhost")
            .await
            .unwrap();

        let result = cache
            .get_or_build("sess_test", &db, "hash1", || async { blocks_clone })
            .await;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text, "Instruction 1");

        // Second call should hit the cache (build closure not called)
        let result2 = cache
            .get_or_build("sess_test", &db, "hash1", || async {
                panic!("should not be called on cache hit")
            })
            .await;
        assert_eq!(result2.len(), 2);
        assert_eq!(result2[0].text, "Instruction 1");
    }

    #[tokio::test]
    async fn test_cache_invalidation_on_hash_change() {
        let cache = PromptCacheManager::new();
        let db = t_koma_db::test_helpers::create_test_ghost_pool("CacheGhost2")
            .await
            .unwrap();

        let blocks_v1 = vec![SystemBlock::new("Version 1")];
        let blocks_v2 = vec![SystemBlock::new("Version 2")];

        let v1 = blocks_v1.clone();
        cache
            .get_or_build("sess_test", &db, "hash_v1", || async { v1 })
            .await;

        // Different hash → cache miss, build is called
        let v2 = blocks_v2.clone();
        let result = cache
            .get_or_build("sess_test", &db, "hash_v2", || async { v2 })
            .await;
        assert_eq!(result[0].text, "Version 2");
    }
}
