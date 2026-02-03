use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    inserted: Instant,
}

#[derive(Debug)]
pub struct TimedCache<K, V> {
    ttl_ms: AtomicU64,
    map: RwLock<HashMap<K, CacheEntry<V>>>,
}

impl<K, V> TimedCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl_ms: AtomicU64::new(ttl.as_millis() as u64),
            map: RwLock::new(HashMap::new()),
        }
    }

    pub fn set_ttl(&self, ttl: Duration) {
        self.ttl_ms.store(ttl.as_millis() as u64, Ordering::Relaxed);
    }

    fn ttl(&self) -> Duration {
        Duration::from_millis(self.ttl_ms.load(Ordering::Relaxed))
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        let ttl = self.ttl();
        let map = self.map.read().await;
        map.get(key).and_then(|entry| {
            if entry.inserted.elapsed() <= ttl {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    pub async fn set(&self, key: K, value: V) {
        let mut map = self.map.write().await;
        map.insert(
            key,
            CacheEntry {
                value,
                inserted: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_get_set() {
        let cache = TimedCache::new(Duration::from_millis(50));
        cache.set("key", "value").await;
        assert_eq!(cache.get(&"key").await, Some("value"));
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let cache = TimedCache::new(Duration::from_millis(10));
        cache.set("key", "value").await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(cache.get(&"key").await, None);
    }
}
