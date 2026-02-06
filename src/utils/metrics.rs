//! Metrics and performance monitoring utilities
//!
//! This module provides metrics collection for monitoring
//! filesystem performance and debugging.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global metrics collector
pub struct Metrics {
    // Read operations
    pub read_ops: AtomicU64,
    pub read_bytes: AtomicU64,
    pub read_cache_hits: AtomicU64,
    pub read_cache_misses: AtomicU64,

    // Write operations
    pub write_ops: AtomicU64,
    pub write_bytes: AtomicU64,
    pub write_buffer_flushes: AtomicU64,

    // Metadata operations
    pub lookup_ops: AtomicU64,
    pub getattr_ops: AtomicU64,
    pub readdir_ops: AtomicU64,
    pub metadata_cache_hits: AtomicU64,
    pub metadata_cache_misses: AtomicU64,

    // Storage operations
    pub obs_get_ops: AtomicU64,
    pub obs_put_ops: AtomicU64,
    pub obs_list_ops: AtomicU64,
    pub obs_delete_ops: AtomicU64,
    pub obs_errors: AtomicU64,

    // Timing
    start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            read_ops: AtomicU64::new(0),
            read_bytes: AtomicU64::new(0),
            read_cache_hits: AtomicU64::new(0),
            read_cache_misses: AtomicU64::new(0),
            write_ops: AtomicU64::new(0),
            write_bytes: AtomicU64::new(0),
            write_buffer_flushes: AtomicU64::new(0),
            lookup_ops: AtomicU64::new(0),
            getattr_ops: AtomicU64::new(0),
            readdir_ops: AtomicU64::new(0),
            metadata_cache_hits: AtomicU64::new(0),
            metadata_cache_misses: AtomicU64::new(0),
            obs_get_ops: AtomicU64::new(0),
            obs_put_ops: AtomicU64::new(0),
            obs_list_ops: AtomicU64::new(0),
            obs_delete_ops: AtomicU64::new(0),
            obs_errors: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Get uptime duration
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Increment read operations
    pub fn inc_read_ops(&self) {
        self.read_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Add read bytes
    pub fn add_read_bytes(&self, bytes: u64) {
        self.read_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Increment read cache hit
    pub fn inc_read_cache_hit(&self) {
        self.read_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment read cache miss
    pub fn inc_read_cache_miss(&self) {
        self.read_cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment write operations
    pub fn inc_write_ops(&self) {
        self.write_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Add write bytes
    pub fn add_write_bytes(&self, bytes: u64) {
        self.write_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Increment write buffer flushes
    pub fn inc_write_buffer_flush(&self) {
        self.write_buffer_flushes.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment lookup operations
    pub fn inc_lookup_ops(&self) {
        self.lookup_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment getattr operations
    pub fn inc_getattr_ops(&self) {
        self.getattr_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment readdir operations
    pub fn inc_readdir_ops(&self) {
        self.readdir_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment metadata cache hit
    pub fn inc_metadata_cache_hit(&self) {
        self.metadata_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment metadata cache miss
    pub fn inc_metadata_cache_miss(&self) {
        self.metadata_cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment OBS GET operations
    pub fn inc_obs_get(&self) {
        self.obs_get_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment OBS PUT operations
    pub fn inc_obs_put(&self) {
        self.obs_put_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment OBS LIST operations
    pub fn inc_obs_list(&self) {
        self.obs_list_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment OBS DELETE operations
    pub fn inc_obs_delete(&self) {
        self.obs_delete_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment OBS errors
    pub fn inc_obs_error(&self) {
        self.obs_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Get read cache hit rate
    pub fn read_cache_hit_rate(&self) -> f64 {
        let hits = self.read_cache_hits.load(Ordering::Relaxed);
        let misses = self.read_cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get metadata cache hit rate
    pub fn metadata_cache_hit_rate(&self) -> f64 {
        let hits = self.metadata_cache_hits.load(Ordering::Relaxed);
        let misses = self.metadata_cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Generate a summary report
    pub fn summary(&self) -> MetricsSummary {
        MetricsSummary {
            uptime: self.uptime(),
            read_ops: self.read_ops.load(Ordering::Relaxed),
            read_bytes: self.read_bytes.load(Ordering::Relaxed),
            read_cache_hit_rate: self.read_cache_hit_rate(),
            write_ops: self.write_ops.load(Ordering::Relaxed),
            write_bytes: self.write_bytes.load(Ordering::Relaxed),
            write_buffer_flushes: self.write_buffer_flushes.load(Ordering::Relaxed),
            lookup_ops: self.lookup_ops.load(Ordering::Relaxed),
            getattr_ops: self.getattr_ops.load(Ordering::Relaxed),
            readdir_ops: self.readdir_ops.load(Ordering::Relaxed),
            metadata_cache_hit_rate: self.metadata_cache_hit_rate(),
            obs_get_ops: self.obs_get_ops.load(Ordering::Relaxed),
            obs_put_ops: self.obs_put_ops.load(Ordering::Relaxed),
            obs_list_ops: self.obs_list_ops.load(Ordering::Relaxed),
            obs_delete_ops: self.obs_delete_ops.load(Ordering::Relaxed),
            obs_errors: self.obs_errors.load(Ordering::Relaxed),
        }
    }
}

/// Summary of collected metrics
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub uptime: Duration,
    pub read_ops: u64,
    pub read_bytes: u64,
    pub read_cache_hit_rate: f64,
    pub write_ops: u64,
    pub write_bytes: u64,
    pub write_buffer_flushes: u64,
    pub lookup_ops: u64,
    pub getattr_ops: u64,
    pub readdir_ops: u64,
    pub metadata_cache_hit_rate: f64,
    pub obs_get_ops: u64,
    pub obs_put_ops: u64,
    pub obs_list_ops: u64,
    pub obs_delete_ops: u64,
    pub obs_errors: u64,
}

impl std::fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== OBS FUSE Metrics ===")?;
        writeln!(f, "Uptime: {:?}", self.uptime)?;
        writeln!(f)?;
        writeln!(f, "Read Operations:")?;
        writeln!(f, "  Total: {}", self.read_ops)?;
        writeln!(f, "  Bytes: {} MB", self.read_bytes / 1024 / 1024)?;
        writeln!(f, "  Cache Hit Rate: {:.2}%", self.read_cache_hit_rate * 100.0)?;
        writeln!(f)?;
        writeln!(f, "Write Operations:")?;
        writeln!(f, "  Total: {}", self.write_ops)?;
        writeln!(f, "  Bytes: {} MB", self.write_bytes / 1024 / 1024)?;
        writeln!(f, "  Buffer Flushes: {}", self.write_buffer_flushes)?;
        writeln!(f)?;
        writeln!(f, "Metadata Operations:")?;
        writeln!(f, "  Lookups: {}", self.lookup_ops)?;
        writeln!(f, "  Getattr: {}", self.getattr_ops)?;
        writeln!(f, "  Readdir: {}", self.readdir_ops)?;
        writeln!(f, "  Cache Hit Rate: {:.2}%", self.metadata_cache_hit_rate * 100.0)?;
        writeln!(f)?;
        writeln!(f, "OBS Operations:")?;
        writeln!(f, "  GET: {}", self.obs_get_ops)?;
        writeln!(f, "  PUT: {}", self.obs_put_ops)?;
        writeln!(f, "  LIST: {}", self.obs_list_ops)?;
        writeln!(f, "  DELETE: {}", self.obs_delete_ops)?;
        writeln!(f, "  Errors: {}", self.obs_errors)?;
        Ok(())
    }
}
