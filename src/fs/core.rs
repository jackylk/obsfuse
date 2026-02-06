//! Core filesystem operations for OBS FUSE
//!
//! This module contains the platform-independent core logic that is shared
//! between the Unix FUSE and Windows WinFSP implementations.

use bytes::Bytes;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::error;

use crate::cache::{
    DataCache, MetadataCache, ReadaheadConfig, ReadaheadManager, WriteBuffer, WriteBufferConfig,
};
use crate::config::Config;
use crate::fs::attr::AttrBuilder;
use crate::fs::dir::DirEntry;
use crate::fs::handle::HandleManager;
use crate::fs::inode::{FileAttr, InodeManager, ROOT_INODE};
use crate::fs::platform::{errno, FileKind};
use crate::storage::ObsClient;
use crate::utils::{Metrics, ObsFuseError};

/// Core filesystem error type
#[derive(Debug)]
pub struct FsError {
    pub errno: i32,
}

impl FsError {
    pub fn new(errno: i32) -> Self {
        Self { errno }
    }

    pub fn not_found() -> Self {
        Self::new(errno::ENOENT)
    }

    pub fn io_error() -> Self {
        Self::new(errno::EIO)
    }

    pub fn not_empty() -> Self {
        Self::new(errno::ENOTEMPTY)
    }
}

impl From<i32> for FsError {
    fn from(errno: i32) -> Self {
        Self::new(errno)
    }
}

/// Result type for core filesystem operations
pub type FsResult<T> = Result<T, FsError>;

/// Core OBS filesystem logic shared between platforms
pub struct ObsFsCore {
    /// Inode manager
    pub inode_mgr: Arc<InodeManager>,
    /// Metadata cache
    pub metadata_cache: Arc<MetadataCache>,
    /// Data cache
    pub data_cache: Arc<DataCache>,
    /// Read-ahead manager
    pub readahead: Arc<ReadaheadManager>,
    /// Write buffer
    pub write_buffer: Arc<WriteBuffer>,
    /// OBS client
    pub obs_client: Arc<ObsClient>,
    /// File handle manager
    pub handle_mgr: Arc<HandleManager>,
    /// Configuration
    pub config: Arc<Config>,
    /// Metrics
    pub metrics: Arc<Metrics>,
    /// Attribute builder (owned, created with 'static lifetime trick)
    attr_builder: AttrBuilder<'static>,
    /// Attribute TTL
    pub attr_ttl: Duration,
    /// Entry TTL
    pub entry_ttl: Duration,
}

impl ObsFsCore {
    /// Create a new OBS filesystem core
    pub fn new(config: Config, metrics: Arc<Metrics>) -> Result<Self, ObsFuseError> {
        let config = Arc::new(config);

        // Create OBS client
        let obs_client = Arc::new(ObsClient::new(config.obs.clone(), metrics.clone())?);

        // Create inode manager
        let inode_mgr = Arc::new(InodeManager::new(config.permission.clone()));

        // Create caches
        let metadata_cache = Arc::new(MetadataCache::new(
            config.cache.metadata.clone(),
            metrics.clone(),
        ));

        let data_cache = Arc::new(DataCache::new(&config.cache, metrics.clone()));

        let readahead_config = ReadaheadConfig::from(&config.performance);
        let readahead = Arc::new(ReadaheadManager::new(readahead_config, metrics.clone()));

        let write_buffer_config = WriteBufferConfig::from(&config.performance);
        let write_buffer = Arc::new(WriteBuffer::new(
            obs_client.clone(),
            write_buffer_config,
            metrics.clone(),
        ));

        // Create handle manager
        let handle_mgr = Arc::new(HandleManager::new());

        // Leak config for static lifetime in AttrBuilder
        // This is safe because config lives for the lifetime of the filesystem
        let config_ref: &'static Config = Box::leak(Box::new((*config).clone()));
        let attr_builder = AttrBuilder::new(&config_ref.permission);

        let attr_ttl = config.cache.metadata.attr_ttl;
        let entry_ttl = config.cache.metadata.dir_ttl;

        Ok(Self {
            inode_mgr,
            metadata_cache,
            data_cache,
            readahead,
            write_buffer,
            obs_client,
            handle_mgr,
            config,
            metrics,
            attr_builder,
            attr_ttl,
            entry_ttl,
        })
    }

    /// Get path for an inode
    pub fn get_path(&self, inode: u64) -> FsResult<String> {
        self.inode_mgr
            .get_path(inode)
            .ok_or_else(FsError::not_found)
    }

    /// Build full OBS path with optional prefix
    pub fn obs_path(&self, path: &str) -> String {
        if let Some(ref prefix) = self.config.obs.prefix {
            if path.is_empty() {
                prefix.clone()
            } else {
                format!("{}/{}", prefix.trim_end_matches('/'), path)
            }
        } else {
            path.to_string()
        }
    }

    /// Lookup a name in a directory
    pub async fn do_lookup(&self, parent: u64, name: &OsStr) -> FsResult<(u64, FileAttr)> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);

        // Check negative cache
        if self.metadata_cache.is_negative(&child_path) {
            return Err(FsError::not_found());
        }

        // Check if already cached
        if let Some(inode) = self.inode_mgr.get_inode(&child_path) {
            if let Some(attr) = self.metadata_cache.get_attr(inode) {
                return Ok((inode, attr));
            }
        }

        // Fetch from OBS
        let obs_path = self.obs_path(&child_path);

        // Try as file first
        match self.obs_client.stat(&obs_path).await {
            Ok(meta) => {
                let inode = self.inode_mgr.get_or_create_inode(&child_path, meta.is_dir, meta.size);
                let attr = self.attr_builder.from_object_meta(inode, &meta);
                self.metadata_cache.put_attr(inode, attr.clone());
                return Ok((inode, attr));
            }
            Err(_) => {}
        }

        // Try as directory
        let dir_path = format!("{}/", obs_path.trim_end_matches('/'));
        match self.obs_client.stat(&dir_path).await {
            Ok(meta) => {
                let inode = self.inode_mgr.get_or_create_inode(&child_path, true, 0);
                let attr = self.attr_builder.from_object_meta(inode, &meta);
                self.metadata_cache.put_attr(inode, attr.clone());
                return Ok((inode, attr));
            }
            Err(_) => {}
        }

        // Check if it's an implicit directory (has children)
        let list_prefix = format!("{}/", obs_path.trim_end_matches('/'));
        match self.obs_client.list(&list_prefix).await {
            Ok(entries) if !entries.is_empty() => {
                let inode = self.inode_mgr.get_or_create_inode(&child_path, true, 0);
                let attr = self.attr_builder.new_directory(inode, None);
                self.metadata_cache.put_attr(inode, attr.clone());
                return Ok((inode, attr));
            }
            _ => {}
        }

        // Not found
        self.metadata_cache.put_negative(&child_path);
        Err(FsError::not_found())
    }

    /// Read directory entries
    pub async fn do_readdir(&self, inode: u64, offset: i64) -> FsResult<Vec<DirEntry>> {
        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        let mut entries = Vec::new();

        // Add . and ..
        if offset < 1 {
            entries.push(DirEntry::dot(inode));
        }
        if offset < 2 {
            let parent_inode = if inode == ROOT_INODE {
                ROOT_INODE
            } else {
                InodeManager::parent_path(&path)
                    .and_then(|p| self.inode_mgr.get_inode(&p))
                    .unwrap_or(ROOT_INODE)
            };
            entries.push(DirEntry::dotdot(parent_inode));
        }

        // List from OBS
        let list_prefix = if obs_path.is_empty() {
            String::new()
        } else {
            format!("{}/", obs_path.trim_end_matches('/'))
        };

        let obs_entries = self
            .obs_client
            .list_dir(&list_prefix)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to list directory");
                FsError::io_error()
            })?;

        let mut child_inodes = Vec::new();
        let mut entry_offset = 3i64;

        for meta in obs_entries {
            if entry_offset <= offset {
                entry_offset += 1;
                continue;
            }

            let name = meta.name();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }

            let child_path = InodeManager::join_path(&path, name);
            let child_inode = self.inode_mgr.get_or_create_inode(&child_path, meta.is_dir, meta.size);

            let kind = if meta.is_dir {
                FileKind::Directory
            } else {
                FileKind::RegularFile
            };

            entries.push(DirEntry::new(child_inode, name, kind, entry_offset));
            child_inodes.push(child_inode);

            // Cache attributes
            let attr = self.attr_builder.from_object_meta(child_inode, &meta);
            self.metadata_cache.put_attr(child_inode, attr);

            entry_offset += 1;
        }

        // Cache children list
        self.inode_mgr.set_children(inode, child_inodes);

        Ok(entries)
    }

    /// Get file attributes
    pub async fn do_getattr(&self, inode: u64) -> FsResult<FileAttr> {
        self.metrics.inc_getattr_ops();

        // Check cache first
        if let Some(attr) = self.metadata_cache.get_attr(inode) {
            return Ok(attr);
        }

        // Fetch from OBS
        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        let meta = self.obs_client.stat(&obs_path).await.map_err(|e| {
            if matches!(e, ObsFuseError::Storage(ref se) if se.kind() == opendal::ErrorKind::NotFound) {
                return FsError::not_found();
            }
            error!(error = %e, "Failed to get attributes");
            FsError::io_error()
        })?;

        let attr = self.attr_builder.from_object_meta(inode, &meta);
        self.metadata_cache.put_attr(inode, attr.clone());

        Ok(attr)
    }

    /// Read from a file
    pub async fn do_read(&self, inode: u64, fh: u64, offset: u64, size: u32) -> FsResult<Bytes> {
        self.metrics.inc_read_ops();

        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        // Update handle position
        self.handle_mgr.update(fh, |state| {
            state.update_position(offset, size as u64);
        });

        // Check read-ahead buffer
        if let Some(data) = self.readahead.get_prefetched(inode, offset) {
            let end = (size as usize).min(data.len());
            return Ok(data.slice(0..end));
        }

        // Check data cache
        if let Some(data) = self.data_cache.get(inode, offset) {
            let start = (offset % self.data_cache.block_size()) as usize;
            let end = (start + size as usize).min(data.len());
            if start < data.len() {
                return Ok(data.slice(start..end));
            }
        }

        // Fetch from OBS
        let data = self
            .obs_client
            .read_range(&obs_path, offset, size as u64)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to read");
                FsError::io_error()
            })?;

        // Cache the data
        self.data_cache.put(inode, offset, data.clone());

        // Trigger read-ahead if sequential
        if let Some(request) = self.readahead.record_read(inode, offset, size as u64) {
            let client = self.obs_client.clone();
            let readahead = self.readahead.clone();
            let path = obs_path.clone();
            tokio::spawn(async move {
                crate::cache::prefetch_task(client, readahead, path, request).await;
            });
        }

        self.metrics.add_read_bytes(data.len() as u64);

        Ok(data)
    }

    /// Write to a file
    pub async fn do_write(&self, inode: u64, fh: u64, offset: u64, data: &[u8]) -> FsResult<usize> {
        self.metrics.inc_write_ops();

        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        // Get current file size
        let file_size = self
            .metadata_cache
            .get_attr(inode)
            .map(|a| a.size)
            .unwrap_or(0);

        // Write to buffer
        let written = self
            .write_buffer
            .write(inode, &obs_path, offset, data, file_size)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to write");
                FsError::io_error()
            })?;

        // Update handle state
        self.handle_mgr.update(fh, |state| {
            state.update_position(offset, written as u64);
            state.mark_dirty();
        });

        // Update cached size
        let new_size = offset + written as u64;
        self.metadata_cache.update_attr(inode, |attr| {
            attr.size = attr.size.max(new_size);
            attr.mtime = SystemTime::now();
        });
        self.inode_mgr.update_size(inode, new_size);

        // Invalidate data cache for written range
        self.data_cache.invalidate_range(inode, offset, written);

        self.metrics.add_write_bytes(written as u64);

        Ok(written)
    }

    /// Create a file
    pub async fn do_create(&self, parent: u64, name: &OsStr, mode: u32, flags: u32) -> FsResult<(u64, FileAttr, u64)> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Create empty file in OBS
        self.obs_client
            .write(&obs_path, Bytes::new())
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to create file");
                FsError::io_error()
            })?;

        // Create inode
        let inode = self.inode_mgr.get_or_create_inode(&child_path, false, 0);
        let attr = self.attr_builder.new_file(inode, Some(mode));
        self.metadata_cache.put_attr(inode, attr.clone());

        // Remove from negative cache
        self.metadata_cache.remove_negative(&child_path);

        // Invalidate parent directory cache
        self.metadata_cache.invalidate_dir(parent);

        // Open handle
        let fh = self.handle_mgr.open(inode, flags, false);

        Ok((inode, attr, fh))
    }

    /// Create a directory
    pub async fn do_mkdir(&self, parent: u64, name: &OsStr, mode: u32) -> FsResult<(u64, FileAttr)> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Create directory marker in OBS
        self.obs_client.create_dir(&obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to create directory");
            FsError::io_error()
        })?;

        // Create inode
        let inode = self.inode_mgr.get_or_create_inode(&child_path, true, 0);
        let attr = self.attr_builder.new_directory(inode, Some(mode));
        self.metadata_cache.put_attr(inode, attr.clone());

        // Remove from negative cache
        self.metadata_cache.remove_negative(&child_path);

        // Invalidate parent directory cache
        self.metadata_cache.invalidate_dir(parent);

        Ok((inode, attr))
    }

    /// Remove a file
    pub async fn do_unlink(&self, parent: u64, name: &OsStr) -> FsResult<()> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Delete from OBS
        self.obs_client.delete(&obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to delete file");
            FsError::io_error()
        })?;

        // Remove inode
        if let Some(inode) = self.inode_mgr.get_inode(&child_path) {
            self.metadata_cache.invalidate(inode);
            self.data_cache.invalidate(inode);
            self.readahead.invalidate(inode);
            self.inode_mgr.remove(inode);
        }

        // Invalidate parent directory cache
        self.metadata_cache.invalidate_dir(parent);

        Ok(())
    }

    /// Remove a directory
    pub async fn do_rmdir(&self, parent: u64, name: &OsStr) -> FsResult<()> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Check if directory is empty
        let list_prefix = format!("{}/", obs_path.trim_end_matches('/'));
        let entries = self.obs_client.list(&list_prefix).await.map_err(|e| {
            error!(error = %e, "Failed to list directory");
            FsError::io_error()
        })?;

        // Filter out the directory marker itself
        let non_marker_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.path != list_prefix && !e.path.ends_with("/."))
            .collect();

        if !non_marker_entries.is_empty() {
            return Err(FsError::not_empty());
        }

        // Delete directory marker
        let dir_marker = format!("{}/", obs_path.trim_end_matches('/'));
        let _ = self.obs_client.delete(&dir_marker).await;

        // Remove inode
        if let Some(inode) = self.inode_mgr.get_inode(&child_path) {
            self.metadata_cache.invalidate(inode);
            self.inode_mgr.remove(inode);
        }

        // Invalidate parent directory cache
        self.metadata_cache.invalidate_dir(parent);

        Ok(())
    }

    /// Rename a file or directory
    pub async fn do_rename(&self, parent: u64, name: &OsStr, new_parent: u64, new_name: &OsStr) -> FsResult<()> {
        let parent_path = self.get_path(parent)?;
        let new_parent_path = self.get_path(new_parent)?;

        let old_path = InodeManager::join_path(&parent_path, &name.to_string_lossy());
        let new_path = InodeManager::join_path(&new_parent_path, &new_name.to_string_lossy());

        let old_obs_path = self.obs_path(&old_path);
        let new_obs_path = self.obs_path(&new_path);

        // Copy then delete (OBS doesn't have native rename)
        self.obs_client
            .copy(&old_obs_path, &new_obs_path)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to copy for rename");
                FsError::io_error()
            })?;

        self.obs_client.delete(&old_obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to delete after rename");
            FsError::io_error()
        })?;

        // Update inode mapping
        self.inode_mgr.rename(&old_path, &new_path);

        // Invalidate caches
        if let Some(inode) = self.inode_mgr.get_inode(&new_path) {
            self.metadata_cache.invalidate(inode);
        }
        self.metadata_cache.invalidate_dir(parent);
        if parent != new_parent {
            self.metadata_cache.invalidate_dir(new_parent);
        }

        Ok(())
    }

    /// Open a file
    pub fn do_open(&self, inode: u64, flags: u32) -> u64 {
        self.handle_mgr.open(inode, flags, false)
    }

    /// Open a directory
    pub fn do_opendir(&self, inode: u64, flags: u32) -> u64 {
        self.handle_mgr.open(inode, flags, true)
    }

    /// Close a handle
    pub fn do_close(&self, fh: u64) {
        self.handle_mgr.close(fh);
    }

    /// Release a file (flush and close)
    pub async fn do_release(&self, inode: u64, fh: u64) -> FsResult<()> {
        // Flush write buffer
        self.write_buffer.release(inode).await.map_err(|e| {
            error!(error = %e, "Failed to flush on release");
            FsError::io_error()
        })?;

        // Close handle
        self.handle_mgr.close(fh);

        // Invalidate metadata cache to ensure consistency
        self.metadata_cache.invalidate_attr(inode);

        Ok(())
    }

    /// Synchronize file contents
    pub async fn do_fsync(&self, inode: u64) -> FsResult<()> {
        self.write_buffer.sync_flush(inode).await.map_err(|e| {
            error!(error = %e, "Failed to fsync");
            FsError::io_error()
        })?;

        Ok(())
    }

    /// Flush file data
    pub async fn do_flush(&self, inode: u64) -> FsResult<()> {
        self.write_buffer.flush(inode).await.map_err(|e| {
            error!(error = %e, "Failed to flush");
            FsError::io_error()
        })?;

        Ok(())
    }

    /// Truncate file
    pub async fn do_truncate(&self, inode: u64, size: u64) -> FsResult<()> {
        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        self.write_buffer.truncate(inode, &obs_path, size).await.map_err(|e| {
            error!(error = %e, "Failed to truncate");
            FsError::io_error()
        })?;

        // Invalidate caches
        self.data_cache.invalidate(inode);
        self.readahead.invalidate(inode);

        Ok(())
    }

    /// Flush all write buffers (for destroy)
    pub async fn flush_all(&self) -> FsResult<()> {
        self.write_buffer.flush_all().await.map_err(|e| {
            error!(error = %e, "Failed to flush write buffers");
            FsError::io_error()
        })?;

        Ok(())
    }

    /// Check file access permissions
    pub fn do_access(&self, inode: u64) -> FsResult<()> {
        if self.inode_mgr.get_entry(inode).is_some() {
            Ok(())
        } else {
            Err(FsError::not_found())
        }
    }
}
