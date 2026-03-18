//! In-memory LRU query cache with TTL.
//!
//! Caches query results to avoid redundant backend calls for identical queries.
//! Thread-safe via tokio::sync::RwLock. No external dependencies.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Cache entry with TTL.
struct CacheEntry {
    data: serde_json::Value,
    native_query: String,
    backend: String,
    backend_type: String,
    signal_type: String,
    inserted_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

/// Cached query result returned to callers.
pub struct CachedResult {
    pub data: serde_json::Value,
    pub native_query: String,
    pub backend: String,
    pub backend_type: String,
    pub signal_type: String,
}

/// Thread-safe in-memory query cache.
pub struct QueryCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    max_entries: usize,
    default_ttl: Duration,
}

impl QueryCache {
    pub fn new(max_entries: usize, default_ttl_secs: u64) -> Self {
        QueryCache {
            entries: RwLock::new(HashMap::new()),
            max_entries,
            default_ttl: Duration::from_secs(default_ttl_secs),
        }
    }

    /// Get a cached result for a query. Returns None if not found or expired.
    pub async fn get(&self, query: &str) -> Option<CachedResult> {
        let entries = self.entries.read().await;
        let entry = entries.get(query)?;
        if entry.is_expired() {
            return None;
        }
        Some(CachedResult {
            data: entry.data.clone(),
            native_query: entry.native_query.clone(),
            backend: entry.backend.clone(),
            backend_type: entry.backend_type.clone(),
            signal_type: entry.signal_type.clone(),
        })
    }

    /// Store a query result in the cache.
    pub async fn put(
        &self,
        query: &str,
        data: serde_json::Value,
        native_query: &str,
        backend: &str,
        backend_type: &str,
        signal_type: &str,
    ) {
        let mut entries = self.entries.write().await;

        // Evict expired entries
        entries.retain(|_, e| !e.is_expired());

        // Evict oldest if at capacity
        if entries.len() >= self.max_entries {
            if let Some(oldest_key) = entries.iter()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest_key);
            }
        }

        entries.insert(query.to_string(), CacheEntry {
            data,
            native_query: native_query.to_string(),
            backend: backend.to_string(),
            backend_type: backend_type.to_string(),
            signal_type: signal_type.to_string(),
            inserted_at: Instant::now(),
            ttl: self.default_ttl,
        });
    }

    /// Cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let entries = self.entries.read().await;
        let total = entries.len();
        let expired = entries.values().filter(|e| e.is_expired()).count();
        CacheStats { total, active: total - expired, expired }
    }

    /// Clear all entries.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }
}

pub struct CacheStats {
    pub total: usize,
    pub active: usize,
    pub expired: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_put_and_get() {
        let cache = QueryCache::new(100, 60);
        cache.put("FROM metrics WHERE __name__ = \"up\"",
            serde_json::json!({"result": []}), "up", "victoria", "prometheus", "metrics").await;

        let result = cache.get("FROM metrics WHERE __name__ = \"up\"").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().native_query, "up");
    }

    #[tokio::test]
    async fn cache_miss_returns_none() {
        let cache = QueryCache::new(100, 60);
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn cache_expired_returns_none() {
        let cache = QueryCache::new(100, 0); // 0 second TTL
        cache.put("q", serde_json::json!({}), "", "", "", "").await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(cache.get("q").await.is_none());
    }

    #[tokio::test]
    async fn cache_evicts_at_capacity() {
        let cache = QueryCache::new(2, 60);
        cache.put("q1", serde_json::json!({}), "", "", "", "").await;
        cache.put("q2", serde_json::json!({}), "", "", "", "").await;
        cache.put("q3", serde_json::json!({}), "", "", "", "").await;

        let stats = cache.stats().await;
        assert!(stats.total <= 2);
    }

    #[tokio::test]
    async fn cache_clear() {
        let cache = QueryCache::new(100, 60);
        cache.put("q1", serde_json::json!({}), "", "", "", "").await;
        cache.put("q2", serde_json::json!({}), "", "", "", "").await;
        cache.clear().await;
        assert_eq!(cache.stats().await.total, 0);
    }

    #[tokio::test]
    async fn cache_stats() {
        let cache = QueryCache::new(100, 60);
        cache.put("q1", serde_json::json!({}), "", "", "", "").await;
        cache.put("q2", serde_json::json!({}), "", "", "", "").await;
        let stats = cache.stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.active, 2);
        assert_eq!(stats.expired, 0);
    }
}
