//! Read-ahead cache for sequential read optimization
//!
//! This module detects sequential read patterns and prefetches
//! data blocks to reduce latency.

use bytes::Bytes;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::{debug, trace};

use crate::config::PerformanceConfig;
use crate::storage::ObsClient;
use crate::utils::Metrics;

/// Read-ahead manager
pub struct ReadaheadManager {
    /// Per-file read state
    file_states: DashMap<u64, ReadState>,
    /// Prefetch buffer
    prefetch_buffer: DashMap<PrefetchKey, PrefetchedData>,
    /// Configuration
    config: ReadaheadConfig,
    /// Concurrency limiter
    semaphore: Arc<Semaphore>,
    /// Metrics
    metrics: Arc<Metrics>,
}

/// Read-ahead configuration
#[derive(Debug, Clone)]
pub struct ReadaheadConfig {
    /// Enable read-ahead
    pub enable: bool,
    /// Window size for prefetching
    pub window_size: u64,
    /// Maximum concurrent prefetch operations
    pub concurrency: usize,
    /// Threshold for detecting sequential reads
    pub seq_threshold: usize,
    /// Prefetch data TTL
    pub prefetch_ttl: Duration,
}

impl Default for ReadaheadConfig {
    fn default() -> Self {
        Self {
            enable: true,
            window_size: 16 * 1024 * 1024, // 16MB
            concurrency: 4,
            seq_threshold: 3,
            prefetch_ttl: Duration::from_secs(30),
        }
    }
}

impl From<&PerformanceConfig> for ReadaheadConfig {
    fn from(config: &PerformanceConfig) -> Self {
        Self {
            enable: config.read_ahead,
            window_size: config.read_ahead_window.as_u64(),
            concurrency: config.read_concurrency,
            seq_threshold: 3,
            prefetch_ttl: Duration::from_secs(30),
        }
    }
}

/// Read state for a file
#[derive(Debug, Clone)]
struct ReadState {
    /// Last read offset
    last_offset: u64,
    /// Last read size
    last_size: u64,
    /// Sequential read count
    seq_count: usize,
    /// Last read time
    last_read: Instant,
    /// Current prefetch offset
    prefetch_offset: u64,
}

impl ReadState {
    fn new() -> Self {
        Self {
            last_offset: 0,
            last_size: 0,
            seq_count: 0,
            last_read: Instant::now(),
            prefetch_offset: 0,
        }
    }

    /// Update state after a read
    fn update(&mut self, offset: u64, size: u64) -> bool {
        let is_sequential = offset == self.last_offset + self.last_size;

        if is_sequential {
            self.seq_count += 1;
        } else {
            self.seq_count = 1;
        }

        self.last_offset = offset;
        self.last_size = size;
        self.last_read = Instant::now();

        is_sequential
    }

    /// Check if sequential pattern detected
    fn is_sequential(&self, threshold: usize) -> bool {
        self.seq_count >= threshold
    }
}

/// Key for prefetch buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PrefetchKey {
    inode: u64,
    offset: u64,
}

/// Prefetched data
#[derive(Debug, Clone)]
struct PrefetchedData {
    data: Bytes,
    fetched_at: Instant,
}

impl PrefetchedData {
    fn new(data: Bytes) -> Self {
        Self {
            data,
            fetched_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

impl ReadaheadManager {
    /// Create a new read-ahead manager
    pub fn new(config: ReadaheadConfig, metrics: Arc<Metrics>) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.concurrency));

        Self {
            file_states: DashMap::new(),
            prefetch_buffer: DashMap::new(),
            config,
            semaphore,
            metrics,
        }
    }

    /// Check if read-ahead is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enable
    }

    /// Record a read operation and check if prefetch should be triggered
    pub fn record_read(&self, inode: u64, offset: u64, size: u64) -> Option<PrefetchRequest> {
        if !self.config.enable {
            return None;
        }

        let mut state = self.file_states.entry(inode).or_insert_with(ReadState::new);
        let is_sequential = state.update(offset, size);

        if is_sequential && state.is_sequential(self.config.seq_threshold) {
            // Calculate prefetch range
            let prefetch_start = offset + size;
            let prefetch_end = prefetch_start + self.config.window_size;

            // Only prefetch if we haven't already
            if prefetch_start > state.prefetch_offset {
                state.prefetch_offset = prefetch_end;

                trace!(
                    inode = inode,
                    start = prefetch_start,
                    end = prefetch_end,
                    "Triggering prefetch"
                );

                return Some(PrefetchRequest {
                    inode,
                    offset: prefetch_start,
                    size: self.config.window_size,
                });
            }
        }

        None
    }

    /// Get prefetched data if available
    pub fn get_prefetched(&self, inode: u64, offset: u64) -> Option<Bytes> {
        let key = PrefetchKey { inode, offset };

        if let Some(entry) = self.prefetch_buffer.get(&key) {
            if !entry.is_expired(self.config.prefetch_ttl) {
                trace!(inode = inode, offset = offset, "Prefetch hit");
                return Some(entry.data.clone());
            }
            // Expired, remove it
            drop(entry);
            self.prefetch_buffer.remove(&key);
        }

        None
    }

    /// Store prefetched data
    pub fn store_prefetched(&self, inode: u64, offset: u64, data: Bytes) {
        let key = PrefetchKey { inode, offset };
        self.prefetch_buffer.insert(key, PrefetchedData::new(data));
        trace!(inode = inode, offset = offset, "Stored prefetched data");
    }

    /// Invalidate prefetch data for an inode
    pub fn invalidate(&self, inode: u64) {
        self.file_states.remove(&inode);

        let keys_to_remove: Vec<_> = self
            .prefetch_buffer
            .iter()
            .filter(|e| e.key().inode == inode)
            .map(|e| *e.key())
            .collect();

        for key in keys_to_remove {
            self.prefetch_buffer.remove(&key);
        }

        debug!(inode = inode, "Invalidated prefetch data");
    }

    /// Clear all prefetch data
    pub fn clear(&self) {
        self.file_states.clear();
        self.prefetch_buffer.clear();
    }

    /// Try to acquire a prefetch permit
    pub fn try_acquire_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        self.semaphore.clone().try_acquire_owned().ok()
    }

    /// Get statistics
    pub fn stats(&self) -> ReadaheadStats {
        ReadaheadStats {
            tracked_files: self.file_states.len(),
            prefetched_blocks: self.prefetch_buffer.len(),
        }
    }
}

/// Prefetch request
#[derive(Debug, Clone)]
pub struct PrefetchRequest {
    pub inode: u64,
    pub offset: u64,
    pub size: u64,
}

/// Read-ahead statistics
#[derive(Debug, Clone)]
pub struct ReadaheadStats {
    pub tracked_files: usize,
    pub prefetched_blocks: usize,
}

/// Background prefetcher task
pub async fn prefetch_task(
    client: Arc<ObsClient>,
    readahead: Arc<ReadaheadManager>,
    path: String,
    request: PrefetchRequest,
) {
    // Try to acquire permit
    let _permit = match readahead.try_acquire_permit() {
        Some(p) => p,
        None => {
            trace!("Prefetch skipped: no permit available");
            return;
        }
    };

    // Fetch data
    match client.read_range(&path, request.offset, request.size).await {
        Ok(data) => {
            readahead.store_prefetched(request.inode, request.offset, data);
        }
        Err(e) => {
            trace!(error = %e, "Prefetch failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_state_sequential() {
        let mut state = ReadState::new();

        // First read
        state.update(0, 1024);
        assert_eq!(state.seq_count, 1);

        // Sequential read
        state.update(1024, 1024);
        assert_eq!(state.seq_count, 2);

        // Another sequential read
        state.update(2048, 1024);
        assert_eq!(state.seq_count, 3);
        assert!(state.is_sequential(3));

        // Non-sequential read
        state.update(0, 1024);
        assert_eq!(state.seq_count, 1);
        assert!(!state.is_sequential(3));
    }

    #[test]
    fn test_readahead_manager() {
        let metrics = Arc::new(Metrics::new());
        let config = ReadaheadConfig::default();
        let manager = ReadaheadManager::new(config, metrics);

        // Simulate sequential reads
        for i in 0..5 {
            let request = manager.record_read(1, i * 1024, 1024);
            if i >= 2 {
                // Should trigger prefetch after threshold
                assert!(request.is_some() || i > 2);
            }
        }
    }

    #[test]
    fn test_prefetch_storage() {
        let metrics = Arc::new(Metrics::new());
        let config = ReadaheadConfig::default();
        let manager = ReadaheadManager::new(config, metrics);

        let data = Bytes::from(vec![0u8; 1024]);
        manager.store_prefetched(1, 0, data.clone());

        let retrieved = manager.get_prefetched(1, 0);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().len(), 1024);

        manager.invalidate(1);
        assert!(manager.get_prefetched(1, 0).is_none());
    }
}
