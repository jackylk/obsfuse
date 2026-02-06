//! Data block cache for OBS FUSE filesystem
//!
//! This module provides two-level caching (memory + disk) for file data blocks.

use bytes::Bytes;
use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, trace, warn};

use crate::config::CacheConfig;
use crate::utils::Metrics;

/// Block key for cache lookup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockKey {
    /// Inode number
    pub inode: u64,
    /// Block offset (aligned to block size)
    pub offset: u64,
}

impl BlockKey {
    /// Create a new block key
    pub fn new(inode: u64, offset: u64, block_size: u64) -> Self {
        Self {
            inode,
            offset: (offset / block_size) * block_size,
        }
    }

    /// Get block number
    pub fn block_num(&self, block_size: u64) -> u64 {
        self.offset / block_size
    }
}

/// Cached block entry
#[derive(Debug, Clone)]
struct CachedBlock {
    /// Block data
    data: Bytes,
    /// When cached
    cached_at: Instant,
    /// Last access time
    last_access: Instant,
    /// Access count
    access_count: u64,
}

impl CachedBlock {
    fn new(data: Bytes) -> Self {
        let now = Instant::now();
        Self {
            data,
            cached_at: now,
            last_access: now,
            access_count: 1,
        }
    }

    fn touch(&mut self) {
        self.last_access = Instant::now();
        self.access_count += 1;
    }
}

/// Data block cache
pub struct DataCache {
    /// Memory cache
    memory_cache: DashMap<BlockKey, CachedBlock>,
    /// Disk cache (optional)
    disk_cache: Option<DiskCache>,
    /// Block size
    block_size: u64,
    /// Memory limit in bytes
    memory_limit: u64,
    /// Current memory usage
    memory_usage: Mutex<u64>,
    /// Metrics
    metrics: Arc<Metrics>,
}

impl DataCache {
    /// Create a new data cache
    pub fn new(config: &CacheConfig, metrics: Arc<Metrics>) -> Self {
        let disk_cache = config.cache_dir.as_ref().map(|dir| {
            DiskCache::new(dir.clone(), config.disk_limit.as_u64())
        });

        Self {
            memory_cache: DashMap::new(),
            disk_cache,
            block_size: config.block_size.as_u64(),
            memory_limit: config.memory_limit.as_u64(),
            memory_usage: Mutex::new(0),
            metrics,
        }
    }

    /// Get block size
    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    /// Get a block from cache
    pub fn get(&self, inode: u64, offset: u64) -> Option<Bytes> {
        let key = BlockKey::new(inode, offset, self.block_size);

        // Try memory cache first
        if let Some(mut entry) = self.memory_cache.get_mut(&key) {
            entry.touch();
            self.metrics.inc_read_cache_hit();
            trace!(inode = inode, offset = offset, "Memory cache hit");
            return Some(entry.data.clone());
        }

        // Try disk cache
        if let Some(ref disk_cache) = self.disk_cache {
            if let Some(data) = disk_cache.get(&key) {
                // Promote to memory cache
                self.put_memory(key, data.clone());
                self.metrics.inc_read_cache_hit();
                trace!(inode = inode, offset = offset, "Disk cache hit");
                return Some(data);
            }
        }

        self.metrics.inc_read_cache_miss();
        trace!(inode = inode, offset = offset, "Cache miss");
        None
    }

    /// Put a block in cache
    pub fn put(&self, inode: u64, offset: u64, data: Bytes) {
        let key = BlockKey::new(inode, offset, self.block_size);
        self.put_memory(key, data);
    }

    /// Put block in memory cache
    fn put_memory(&self, key: BlockKey, data: Bytes) {
        let data_size = data.len() as u64;

        // Check if we need to evict
        {
            let mut usage = self.memory_usage.lock();
            while *usage + data_size > self.memory_limit && !self.memory_cache.is_empty() {
                if let Some(evicted_size) = self.evict_one() {
                    *usage = usage.saturating_sub(evicted_size);
                } else {
                    break;
                }
            }
            *usage += data_size;
        }

        self.memory_cache.insert(key, CachedBlock::new(data));
        trace!(
            inode = key.inode,
            offset = key.offset,
            "Cached block in memory"
        );
    }

    /// Evict one block (LRU)
    fn evict_one(&self) -> Option<u64> {
        // Find least recently used block
        let mut oldest_key = None;
        let mut oldest_time = Instant::now();

        for entry in self.memory_cache.iter() {
            if entry.value().last_access < oldest_time {
                oldest_time = entry.value().last_access;
                oldest_key = Some(*entry.key());
            }
        }

        if let Some(key) = oldest_key {
            if let Some((_, block)) = self.memory_cache.remove(&key) {
                let size = block.data.len() as u64;

                // Optionally write to disk cache
                if let Some(ref disk_cache) = self.disk_cache {
                    disk_cache.put(&key, &block.data);
                }

                trace!(
                    inode = key.inode,
                    offset = key.offset,
                    "Evicted block from memory"
                );
                return Some(size);
            }
        }

        None
    }

    /// Invalidate blocks for an inode
    pub fn invalidate(&self, inode: u64) {
        let keys_to_remove: Vec<_> = self
            .memory_cache
            .iter()
            .filter(|e| e.key().inode == inode)
            .map(|e| *e.key())
            .collect();

        let mut removed_size = 0u64;
        for key in keys_to_remove {
            if let Some((_, block)) = self.memory_cache.remove(&key) {
                removed_size += block.data.len() as u64;
            }
        }

        {
            let mut usage = self.memory_usage.lock();
            *usage = usage.saturating_sub(removed_size);
        }

        // Also invalidate disk cache
        if let Some(ref disk_cache) = self.disk_cache {
            disk_cache.invalidate(inode);
        }

        debug!(inode = inode, "Invalidated data cache");
    }

    /// Invalidate a range of blocks
    pub fn invalidate_range(&self, inode: u64, offset: u64, len: usize) {
        let start_block = offset / self.block_size;
        let end_block = (offset + len as u64 + self.block_size - 1) / self.block_size;

        let mut removed_size = 0u64;
        for block_num in start_block..end_block {
            let key = BlockKey {
                inode,
                offset: block_num * self.block_size,
            };
            if let Some((_, block)) = self.memory_cache.remove(&key) {
                removed_size += block.data.len() as u64;
            }
        }

        {
            let mut usage = self.memory_usage.lock();
            *usage = usage.saturating_sub(removed_size);
        }

        trace!(
            inode = inode,
            offset = offset,
            len = len,
            "Invalidated range"
        );
    }

    /// Clear all cache
    pub fn clear(&self) {
        self.memory_cache.clear();
        *self.memory_usage.lock() = 0;

        if let Some(ref disk_cache) = self.disk_cache {
            disk_cache.clear();
        }

        debug!("Cleared all data cache");
    }

    /// Get cache statistics
    pub fn stats(&self) -> DataCacheStats {
        DataCacheStats {
            memory_entries: self.memory_cache.len(),
            memory_usage: *self.memory_usage.lock(),
            memory_limit: self.memory_limit,
            disk_entries: self.disk_cache.as_ref().map(|d| d.entry_count()).unwrap_or(0),
        }
    }
}

/// Disk-based cache for overflow
struct DiskCache {
    /// Cache directory
    cache_dir: PathBuf,
    /// Size limit
    size_limit: u64,
    /// Entry tracking
    entries: Mutex<HashMap<BlockKey, DiskCacheEntry>>,
    /// Current size
    current_size: Mutex<u64>,
}

#[derive(Debug, Clone)]
struct DiskCacheEntry {
    path: PathBuf,
    size: u64,
    last_access: Instant,
}

impl DiskCache {
    fn new(cache_dir: PathBuf, size_limit: u64) -> Self {
        // Create cache directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&cache_dir) {
            warn!(error = %e, "Failed to create disk cache directory");
        }

        Self {
            cache_dir,
            size_limit,
            entries: Mutex::new(HashMap::new()),
            current_size: Mutex::new(0),
        }
    }

    fn get(&self, key: &BlockKey) -> Option<Bytes> {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(key) {
            entry.last_access = Instant::now();
            let path = entry.path.clone();
            drop(entries);

            match fs::read(&path) {
                Ok(data) => Some(Bytes::from(data)),
                Err(e) => {
                    warn!(error = %e, "Failed to read from disk cache");
                    None
                }
            }
        } else {
            None
        }
    }

    fn put(&self, key: &BlockKey, data: &Bytes) {
        let filename = format!("{}_{}", key.inode, key.offset);
        let path = self.cache_dir.join(&filename);
        let size = data.len() as u64;

        // Check size limit
        {
            let mut current = self.current_size.lock();
            while *current + size > self.size_limit {
                if !self.evict_one_locked() {
                    return; // Can't evict, don't cache
                }
                *current = self.calculate_size();
            }
        }

        // Write to disk
        match File::create(&path).and_then(|mut f| f.write_all(data)) {
            Ok(_) => {
                let mut entries = self.entries.lock();
                entries.insert(
                    *key,
                    DiskCacheEntry {
                        path,
                        size,
                        last_access: Instant::now(),
                    },
                );
                *self.current_size.lock() += size;
            }
            Err(e) => {
                warn!(error = %e, "Failed to write to disk cache");
            }
        }
    }

    fn invalidate(&self, inode: u64) {
        let mut entries = self.entries.lock();
        let keys_to_remove: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.inode == inode)
            .map(|(k, _)| *k)
            .collect();

        for key in keys_to_remove {
            if let Some(entry) = entries.remove(&key) {
                let _ = fs::remove_file(&entry.path);
            }
        }
    }

    fn clear(&self) {
        let mut entries = self.entries.lock();
        for (_, entry) in entries.drain() {
            let _ = fs::remove_file(&entry.path);
        }
        *self.current_size.lock() = 0;
    }

    fn evict_one_locked(&self) -> bool {
        let mut entries = self.entries.lock();

        // Find oldest entry
        let oldest = entries
            .iter()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(k, _)| *k);

        if let Some(key) = oldest {
            if let Some(entry) = entries.remove(&key) {
                let _ = fs::remove_file(&entry.path);
                return true;
            }
        }

        false
    }

    fn calculate_size(&self) -> u64 {
        self.entries.lock().values().map(|e| e.size).sum()
    }

    fn entry_count(&self) -> usize {
        self.entries.lock().len()
    }
}

/// Data cache statistics
#[derive(Debug, Clone)]
pub struct DataCacheStats {
    pub memory_entries: usize,
    pub memory_usage: u64,
    pub memory_limit: u64,
    pub disk_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytesize::ByteSize;

    fn test_config() -> CacheConfig {
        CacheConfig {
            memory_limit: ByteSize::mb(10),
            disk_limit: ByteSize::mb(100),
            cache_dir: None,
            block_size: ByteSize::kb(64),
            metadata: Default::default(),
        }
    }

    #[test]
    fn test_block_key() {
        let key = BlockKey::new(1, 100, 64);
        assert_eq!(key.inode, 1);
        assert_eq!(key.offset, 64); // Aligned to block size

        let key = BlockKey::new(1, 64, 64);
        assert_eq!(key.offset, 64);

        let key = BlockKey::new(1, 128, 64);
        assert_eq!(key.offset, 128);
    }

    #[test]
    fn test_data_cache() {
        let metrics = Arc::new(Metrics::new());
        let cache = DataCache::new(&test_config(), metrics);

        let data = Bytes::from(vec![0u8; 1024]);
        cache.put(1, 0, data.clone());

        let cached = cache.get(1, 0);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1024);

        cache.invalidate(1);
        assert!(cache.get(1, 0).is_none());
    }

    #[test]
    fn test_cache_eviction() {
        let mut config = test_config();
        config.memory_limit = ByteSize::kb(10); // Small limit

        let metrics = Arc::new(Metrics::new());
        let cache = DataCache::new(&config, metrics);

        // Add blocks until eviction happens
        for i in 0..20 {
            let data = Bytes::from(vec![i as u8; 1024]);
            cache.put(1, i * 1024, data);
        }

        // Some blocks should have been evicted
        let stats = cache.stats();
        assert!(stats.memory_usage <= config.memory_limit.as_u64());
    }
}
