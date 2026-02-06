//! Metadata cache for OBS FUSE filesystem
//!
//! This module provides caching for file attributes and directory listings
//! with TTL-based expiration for strong consistency.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

use crate::config::MetadataCacheConfig;
use crate::fs::inode::FileAttr;
use crate::utils::Metrics;

/// Metadata cache
pub struct MetadataCache {
    /// Attribute cache (inode -> cached attr)
    attr_cache: DashMap<u64, CachedAttr>,
    /// Directory listing cache (inode -> cached entries)
    dir_cache: DashMap<u64, CachedDirEntry>,
    /// Negative cache (path -> timestamp)
    negative_cache: DashMap<String, Instant>,
    /// Configuration
    config: MetadataCacheConfig,
    /// Metrics
    metrics: Arc<Metrics>,
}

/// Cached file attributes
#[derive(Debug, Clone)]
pub struct CachedAttr {
    /// File attributes
    pub attr: FileAttr,
    /// When cached
    pub cached_at: Instant,
}

impl CachedAttr {
    /// Create new cached attribute
    pub fn new(attr: FileAttr) -> Self {
        Self {
            attr,
            cached_at: Instant::now(),
        }
    }

    /// Check if expired
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }
}

/// Cached directory entry
#[derive(Debug, Clone)]
pub struct CachedDirEntry {
    /// Child inodes
    pub children: Vec<u64>,
    /// When cached
    pub cached_at: Instant,
    /// Whether listing is complete
    pub complete: bool,
}

impl CachedDirEntry {
    /// Create new cached directory entry
    pub fn new(children: Vec<u64>, complete: bool) -> Self {
        Self {
            children,
            cached_at: Instant::now(),
            complete,
        }
    }

    /// Check if expired
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }
}

impl MetadataCache {
    /// Create a new metadata cache
    pub fn new(config: MetadataCacheConfig, metrics: Arc<Metrics>) -> Self {
        Self {
            attr_cache: DashMap::new(),
            dir_cache: DashMap::new(),
            negative_cache: DashMap::new(),
            config,
            metrics,
        }
    }

    /// Get cached attributes
    pub fn get_attr(&self, inode: u64) -> Option<FileAttr> {
        if let Some(cached) = self.attr_cache.get(&inode) {
            if !cached.is_expired(self.config.attr_ttl) {
                self.metrics.inc_metadata_cache_hit();
                trace!(inode = inode, "Metadata cache hit");
                return Some(cached.attr.clone());
            }
            // Expired, remove it
            drop(cached);
            self.attr_cache.remove(&inode);
        }
        self.metrics.inc_metadata_cache_miss();
        trace!(inode = inode, "Metadata cache miss");
        None
    }

    /// Put attributes in cache
    pub fn put_attr(&self, inode: u64, attr: FileAttr) {
        // Check cache size limit
        if self.attr_cache.len() >= self.config.max_entries {
            self.evict_oldest_attrs(self.config.max_entries / 10);
        }

        self.attr_cache.insert(inode, CachedAttr::new(attr));
        trace!(inode = inode, "Cached attributes");
    }

    /// Update cached attributes
    pub fn update_attr<F>(&self, inode: u64, f: F)
    where
        F: FnOnce(&mut FileAttr),
    {
        if let Some(mut cached) = self.attr_cache.get_mut(&inode) {
            f(&mut cached.attr);
            cached.cached_at = Instant::now();
        }
    }

    /// Invalidate cached attributes
    pub fn invalidate_attr(&self, inode: u64) {
        self.attr_cache.remove(&inode);
        debug!(inode = inode, "Invalidated attribute cache");
    }

    /// Get cached directory listing
    pub fn get_dir(&self, inode: u64) -> Option<Vec<u64>> {
        if let Some(cached) = self.dir_cache.get(&inode) {
            if !cached.is_expired(self.config.dir_ttl) {
                self.metrics.inc_metadata_cache_hit();
                trace!(inode = inode, "Directory cache hit");
                return Some(cached.children.clone());
            }
            // Expired, remove it
            drop(cached);
            self.dir_cache.remove(&inode);
        }
        self.metrics.inc_metadata_cache_miss();
        trace!(inode = inode, "Directory cache miss");
        None
    }

    /// Put directory listing in cache
    pub fn put_dir(&self, inode: u64, children: Vec<u64>, complete: bool) {
        self.dir_cache
            .insert(inode, CachedDirEntry::new(children, complete));
        trace!(inode = inode, "Cached directory listing");
    }

    /// Invalidate cached directory listing
    pub fn invalidate_dir(&self, inode: u64) {
        self.dir_cache.remove(&inode);
        debug!(inode = inode, "Invalidated directory cache");
    }

    /// Check negative cache (non-existent path)
    pub fn is_negative(&self, path: &str) -> bool {
        if let Some(timestamp) = self.negative_cache.get(path) {
            if timestamp.elapsed() <= self.config.negative_ttl {
                trace!(path = path, "Negative cache hit");
                return true;
            }
            // Expired
            drop(timestamp);
            self.negative_cache.remove(path);
        }
        false
    }

    /// Add to negative cache
    pub fn put_negative(&self, path: &str) {
        self.negative_cache.insert(path.to_string(), Instant::now());
        trace!(path = path, "Added to negative cache");
    }

    /// Remove from negative cache
    pub fn remove_negative(&self, path: &str) {
        self.negative_cache.remove(path);
    }

    /// Invalidate all caches for an inode
    pub fn invalidate(&self, inode: u64) {
        self.invalidate_attr(inode);
        self.invalidate_dir(inode);
    }

    /// Invalidate parent directory cache
    pub fn invalidate_parent(&self, parent_inode: u64) {
        self.invalidate_dir(parent_inode);
    }

    /// Clear all caches
    pub fn clear(&self) {
        self.attr_cache.clear();
        self.dir_cache.clear();
        self.negative_cache.clear();
        debug!("Cleared all metadata caches");
    }

    /// Evict oldest attribute entries
    fn evict_oldest_attrs(&self, count: usize) {
        let mut entries: Vec<_> = self
            .attr_cache
            .iter()
            .map(|e| (*e.key(), e.value().cached_at))
            .collect();

        entries.sort_by_key(|(_, time)| *time);

        for (inode, _) in entries.into_iter().take(count) {
            self.attr_cache.remove(&inode);
        }

        debug!(count = count, "Evicted oldest attribute entries");
    }

    /// Get cache statistics
    pub fn stats(&self) -> MetadataCacheStats {
        MetadataCacheStats {
            attr_entries: self.attr_cache.len(),
            dir_entries: self.dir_cache.len(),
            negative_entries: self.negative_cache.len(),
        }
    }
}

/// Metadata cache statistics
#[derive(Debug, Clone)]
pub struct MetadataCacheStats {
    pub attr_entries: usize,
    pub dir_entries: usize,
    pub negative_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuse3::FileType;

    fn test_config() -> MetadataCacheConfig {
        MetadataCacheConfig {
            attr_ttl: Duration::from_secs(3),
            dir_ttl: Duration::from_secs(5),
            negative_ttl: Duration::from_secs(1),
            max_entries: 1000,
        }
    }

    #[test]
    fn test_attr_cache() {
        let metrics = Arc::new(Metrics::new());
        let cache = MetadataCache::new(test_config(), metrics);

        let attr = FileAttr {
            ino: 42,
            size: 1024,
            kind: FileType::RegularFile,
            ..Default::default()
        };

        cache.put_attr(42, attr.clone());

        let cached = cache.get_attr(42);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().size, 1024);

        cache.invalidate_attr(42);
        assert!(cache.get_attr(42).is_none());
    }

    #[test]
    fn test_dir_cache() {
        let metrics = Arc::new(Metrics::new());
        let cache = MetadataCache::new(test_config(), metrics);

        cache.put_dir(1, vec![2, 3, 4], true);

        let cached = cache.get_dir(1);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), vec![2, 3, 4]);

        cache.invalidate_dir(1);
        assert!(cache.get_dir(1).is_none());
    }

    #[test]
    fn test_negative_cache() {
        let metrics = Arc::new(Metrics::new());
        let cache = MetadataCache::new(test_config(), metrics);

        cache.put_negative("/nonexistent");
        assert!(cache.is_negative("/nonexistent"));

        cache.remove_negative("/nonexistent");
        assert!(!cache.is_negative("/nonexistent"));
    }
}
