//! Write buffer for batching and optimizing write operations
//!
//! This module provides write buffering to reduce API calls and
//! support efficient large file uploads via multipart.

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

use crate::config::PerformanceConfig;
use crate::storage::{MultipartConfig, MultipartUploadState, MultipartUploader, ObsClient};
use crate::utils::{Metrics, Result};

/// Write buffer manager
pub struct WriteBuffer {
    /// Per-file buffers
    buffers: DashMap<u64, Arc<Mutex<FileWriteBuffer>>>,
    /// OBS client
    client: Arc<ObsClient>,
    /// Multipart uploader
    multipart: MultipartUploader,
    /// Configuration
    config: WriteBufferConfig,
    /// Metrics
    metrics: Arc<Metrics>,
}

/// Write buffer configuration
#[derive(Debug, Clone)]
pub struct WriteBufferConfig {
    /// Buffer size per file
    pub buffer_size: usize,
    /// Auto-flush interval
    pub flush_interval: Duration,
    /// Threshold for multipart upload
    pub multipart_threshold: usize,
    /// Part size for multipart upload
    pub part_size: usize,
}

impl Default for WriteBufferConfig {
    fn default() -> Self {
        Self {
            buffer_size: 64 * 1024 * 1024,      // 64MB
            flush_interval: Duration::from_secs(30),
            multipart_threshold: 100 * 1024 * 1024, // 100MB
            part_size: 8 * 1024 * 1024,         // 8MB
        }
    }
}

impl From<&PerformanceConfig> for WriteBufferConfig {
    fn from(config: &PerformanceConfig) -> Self {
        Self {
            buffer_size: config.write_buffer_size.as_u64() as usize,
            flush_interval: config.flush_interval,
            multipart_threshold: config.multipart_threshold.as_u64() as usize,
            part_size: config.multipart_part_size.as_u64() as usize,
        }
    }
}

/// Buffer for a single file
#[derive(Debug)]
pub struct FileWriteBuffer {
    /// Inode
    pub inode: u64,
    /// Object path
    pub path: String,
    /// Buffered data
    pub data: BytesMut,
    /// Whether buffer has been modified
    pub dirty: bool,
    /// Last write time
    pub last_write: Instant,
    /// File size (including unflushed data)
    pub size: u64,
    /// Original file size (before writes)
    pub original_size: u64,
    /// Multipart upload state (for large files)
    pub multipart: Option<MultipartUploadState>,
    /// Whether file is being written sequentially from start
    pub sequential_write: bool,
    /// Next expected write offset
    pub next_offset: u64,
}

impl FileWriteBuffer {
    /// Create a new file write buffer
    pub fn new(inode: u64, path: String, original_size: u64) -> Self {
        Self {
            inode,
            path,
            data: BytesMut::new(),
            dirty: false,
            last_write: Instant::now(),
            size: original_size,
            original_size,
            multipart: None,
            sequential_write: true,
            next_offset: 0,
        }
    }

    /// Check if buffer needs flush based on size
    pub fn needs_flush(&self, threshold: usize) -> bool {
        self.data.len() >= threshold
    }

    /// Check if buffer needs flush based on time
    pub fn needs_time_flush(&self, interval: Duration) -> bool {
        self.dirty && self.last_write.elapsed() > interval
    }

    /// Get current buffer size
    pub fn buffer_size(&self) -> usize {
        self.data.len()
    }
}

impl WriteBuffer {
    /// Create a new write buffer manager
    pub fn new(
        client: Arc<ObsClient>,
        config: WriteBufferConfig,
        metrics: Arc<Metrics>,
    ) -> Self {
        let multipart_config = MultipartConfig {
            part_size: config.part_size,
            concurrency: 5,
            max_parts: 10000,
        };

        let multipart = MultipartUploader::new(
            Arc::new(client.operator().clone()),
            multipart_config,
            metrics.clone(),
        );

        Self {
            buffers: DashMap::new(),
            client,
            multipart,
            config,
            metrics,
        }
    }

    /// Get or create buffer for a file
    pub fn get_or_create(&self, inode: u64, path: &str, size: u64) -> Arc<Mutex<FileWriteBuffer>> {
        self.buffers
            .entry(inode)
            .or_insert_with(|| {
                Arc::new(Mutex::new(FileWriteBuffer::new(inode, path.to_string(), size)))
            })
            .clone()
    }

    /// Write data to buffer
    pub async fn write(
        &self,
        inode: u64,
        path: &str,
        offset: u64,
        data: &[u8],
        file_size: u64,
    ) -> Result<usize> {
        let buffer = self.get_or_create(inode, path, file_size);
        let mut buf = buffer.lock().await;

        // Check if this is a sequential write
        if offset != buf.next_offset {
            buf.sequential_write = false;
        }
        buf.next_offset = offset + data.len() as u64;

        // For non-sequential writes or writes to existing files with data,
        // we need to handle more carefully
        if !buf.sequential_write && buf.original_size > 0 {
            // Flush current buffer and do direct write
            // This is a simplified approach - a full implementation would
            // handle sparse writes properly
            self.flush_buffer(&mut buf).await?;
            self.client.write(path, Bytes::copy_from_slice(data)).await?;
            buf.size = buf.size.max(offset + data.len() as u64);
            return Ok(data.len());
        }

        // Append to buffer
        buf.data.extend_from_slice(data);
        buf.dirty = true;
        buf.last_write = Instant::now();
        buf.size = buf.size.max(offset + data.len() as u64);

        trace!(
            inode = inode,
            offset = offset,
            len = data.len(),
            buffer_size = buf.data.len(),
            "Buffered write"
        );

        // Check if we need to start multipart upload
        if buf.data.len() >= self.config.multipart_threshold && buf.multipart.is_none() {
            buf.multipart = Some(MultipartUploadState::new(path.to_string()));
            debug!(inode = inode, "Started multipart upload");
        }

        // Flush parts if using multipart
        if buf.multipart.is_some() {
            let part_size = self.config.part_size;
            while buf.data.len() >= part_size {
                let part_data: Vec<u8> = buf.data.split_to(part_size).to_vec();
                if let Some(ref mut mp_state) = buf.multipart {
                    self.multipart
                        .write(mp_state, &part_data)
                        .await?;
                }
            }
        }

        // Check if buffer needs flush
        if buf.needs_flush(self.config.buffer_size) && buf.multipart.is_none() {
            self.flush_buffer(&mut buf).await?;
        }

        Ok(data.len())
    }

    /// Flush buffer for a file
    pub async fn flush(&self, inode: u64) -> Result<()> {
        if let Some(buffer) = self.buffers.get(&inode) {
            let mut buf = buffer.lock().await;
            self.flush_buffer(&mut buf).await?;
        }
        Ok(())
    }

    /// Synchronous flush (waits for completion)
    pub async fn sync_flush(&self, inode: u64) -> Result<()> {
        self.flush(inode).await?;
        self.metrics.inc_write_buffer_flush();
        Ok(())
    }

    /// Internal flush implementation
    async fn flush_buffer(&self, buf: &mut FileWriteBuffer) -> Result<()> {
        if !buf.dirty || buf.data.is_empty() {
            return Ok(());
        }

        if let Some(ref mut mp_state) = buf.multipart {
            // Complete multipart upload
            // First, upload any remaining data as final part
            if !buf.data.is_empty() {
                let remaining = buf.data.split().freeze();
                self.multipart.write(mp_state, &remaining).await?;
            }
            self.multipart.complete(mp_state).await?;
            buf.multipart = None;
            debug!(inode = buf.inode, "Completed multipart upload");
        } else {
            // Single PUT for small files
            let data = buf.data.split().freeze();
            self.client.write(&buf.path, data).await?;
            debug!(inode = buf.inode, size = buf.size, "Flushed buffer");
        }

        buf.dirty = false;
        buf.original_size = buf.size;
        self.metrics.inc_write_buffer_flush();

        Ok(())
    }

    /// Release buffer for a file (flush and remove)
    pub async fn release(&self, inode: u64) -> Result<()> {
        if let Some((_, buffer)) = self.buffers.remove(&inode) {
            let mut buf = buffer.lock().await;
            self.flush_buffer(&mut buf).await?;
            debug!(inode = inode, "Released write buffer");
        }
        Ok(())
    }

    /// Truncate file
    pub async fn truncate(&self, inode: u64, path: &str, size: u64) -> Result<()> {
        let buffer = self.get_or_create(inode, path, size);
        let mut buf = buffer.lock().await;

        // Abort any multipart upload
        if let Some(ref mut mp_state) = buf.multipart {
            self.multipart.abort(mp_state).await?;
            buf.multipart = None;
        }

        // Clear buffer
        buf.data.clear();
        buf.size = size;
        buf.dirty = true;

        if size == 0 {
            // Write empty file
            self.client.write(path, Bytes::new()).await?;
            buf.dirty = false;
        }

        Ok(())
    }

    /// Get all dirty buffers
    pub fn dirty_buffers(&self) -> Vec<u64> {
        self.buffers
            .iter()
            .filter_map(|entry| {
                // We can't easily check dirty status without locking
                // Return all for now
                Some(*entry.key())
            })
            .collect()
    }

    /// Flush all dirty buffers
    pub async fn flush_all(&self) -> Result<()> {
        for entry in self.buffers.iter() {
            let mut buf = entry.value().lock().await;
            if buf.dirty {
                self.flush_buffer(&mut buf).await?;
            }
        }
        Ok(())
    }

    /// Flush buffers that need time-based flush
    pub async fn flush_expired(&self) -> Result<()> {
        for entry in self.buffers.iter() {
            let mut buf = entry.value().lock().await;
            if buf.needs_time_flush(self.config.flush_interval) {
                self.flush_buffer(&mut buf).await?;
            }
        }
        Ok(())
    }

    /// Get buffer statistics
    pub fn stats(&self) -> WriteBufferStats {
        let mut total_size = 0;
        let mut dirty_count = 0;

        for entry in self.buffers.iter() {
            if let Ok(buf) = entry.value().try_lock() {
                total_size += buf.data.len();
                if buf.dirty {
                    dirty_count += 1;
                }
            }
        }

        WriteBufferStats {
            buffer_count: self.buffers.len(),
            total_size,
            dirty_count,
        }
    }
}

/// Write buffer statistics
#[derive(Debug, Clone)]
pub struct WriteBufferStats {
    pub buffer_count: usize,
    pub total_size: usize,
    pub dirty_count: usize,
}

/// Background flush task
pub async fn flush_task(write_buffer: Arc<WriteBuffer>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;

        if let Err(e) = write_buffer.flush_expired().await {
            warn!(error = %e, "Background flush failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_write_buffer() {
        let mut buf = FileWriteBuffer::new(1, "test.txt".to_string(), 0);

        assert!(!buf.dirty);
        assert_eq!(buf.buffer_size(), 0);

        buf.data.extend_from_slice(&[0u8; 1024]);
        buf.dirty = true;

        assert!(buf.dirty);
        assert_eq!(buf.buffer_size(), 1024);
        assert!(!buf.needs_flush(2048));
        assert!(buf.needs_flush(512));
    }

    #[test]
    fn test_write_buffer_config() {
        let config = WriteBufferConfig::default();
        assert_eq!(config.buffer_size, 64 * 1024 * 1024);
        assert_eq!(config.multipart_threshold, 100 * 1024 * 1024);
    }
}
