//! Main OBS FUSE filesystem implementation
//!
//! This module implements the fuse3 Filesystem trait to provide
//! a POSIX-compatible interface to OBS object storage.

use bytes::Bytes;
use fuse3::raw::prelude::*;
use fuse3::{Errno, FileType, Inode, Result as FuseResult, SetAttr, Timestamp};
use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{error, info, instrument};

use crate::cache::{
    DataCache, MetadataCache, ReadaheadConfig, ReadaheadManager, WriteBuffer, WriteBufferConfig,
};
use crate::config::Config;
use crate::fs::attr::AttrBuilder;
use crate::fs::dir::DirEntry;
use crate::fs::handle::HandleManager;
use crate::fs::inode::{FileAttr, InodeManager, ROOT_INODE};
use crate::storage::ObsClient;
use crate::utils::{Metrics, ObsFuseError};

/// OBS FUSE filesystem
pub struct ObsFs {
    /// Inode manager
    inode_mgr: Arc<InodeManager>,
    /// Metadata cache
    metadata_cache: Arc<MetadataCache>,
    /// Data cache
    data_cache: Arc<DataCache>,
    /// Read-ahead manager
    readahead: Arc<ReadaheadManager>,
    /// Write buffer
    write_buffer: Arc<WriteBuffer>,
    /// OBS client
    obs_client: Arc<ObsClient>,
    /// File handle manager
    handle_mgr: Arc<HandleManager>,
    /// Configuration
    config: Arc<Config>,
    /// Metrics
    metrics: Arc<Metrics>,
    /// Attribute builder
    attr_builder: AttrBuilder<'static>,
    /// Attribute TTL
    attr_ttl: Duration,
    /// Entry TTL
    entry_ttl: Duration,
}

impl ObsFs {
    /// Create a new OBS filesystem
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
    fn get_path(&self, inode: u64) -> Result<String, Errno> {
        self.inode_mgr
            .get_path(inode)
            .ok_or(Errno::from(libc::ENOENT))
    }

    /// Build full OBS path with optional prefix
    fn obs_path(&self, path: &str) -> String {
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
    async fn do_lookup(&self, parent: u64, name: &OsStr) -> Result<(u64, FileAttr), Errno> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);

        // Check negative cache
        if self.metadata_cache.is_negative(&child_path) {
            return Err(Errno::from(libc::ENOENT));
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
        Err(Errno::from(libc::ENOENT))
    }

    /// Read directory entries
    async fn do_readdir(&self, inode: u64, offset: i64) -> Result<Vec<DirEntry>, Errno> {
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
                Errno::from(libc::EIO)
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
                FileType::Directory
            } else {
                FileType::RegularFile
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
}

/// Convert fuse3 Timestamp to SystemTime
fn timestamp_to_systemtime(ts: Timestamp) -> SystemTime {
    let duration = std::time::Duration::new(ts.sec as u64, ts.nsec);
    if ts.sec >= 0 {
        std::time::UNIX_EPOCH + duration
    } else {
        std::time::UNIX_EPOCH
    }
}

impl Filesystem for ObsFs {
    type DirEntryStream<'a> = futures::stream::Iter<std::vec::IntoIter<FuseResult<DirectoryEntry>>> where Self: 'a;
    type DirEntryPlusStream<'a> = futures::stream::Iter<std::vec::IntoIter<FuseResult<DirectoryEntryPlus>>> where Self: 'a;

    /// Initialize filesystem
    #[instrument(skip(self, _req), level = "debug")]
    async fn init(&self, _req: Request) -> FuseResult<ReplyInit> {
        info!("Initializing OBS FUSE filesystem");
        Ok(ReplyInit {
            max_write: NonZeroU32::new(self.config.fuse.max_write.as_u64() as u32).unwrap(),
        })
    }

    /// Clean up filesystem
    #[instrument(skip(self, _req), level = "debug")]
    async fn destroy(&self, _req: Request) {
        info!("Destroying OBS FUSE filesystem");

        // Flush all write buffers
        if let Err(e) = self.write_buffer.flush_all().await {
            error!(error = %e, "Failed to flush write buffers on destroy");
        }
    }

    /// Look up a directory entry
    #[instrument(skip(self, _req), level = "debug")]
    async fn lookup(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<ReplyEntry> {
        self.metrics.inc_lookup_ops();

        let (_inode, attr) = self.do_lookup(parent, name).await?;

        Ok(ReplyEntry {
            ttl: self.entry_ttl,
            attr: attr.to_fuse3(),
            generation: 0,
        })
    }

    /// Get file attributes
    #[instrument(skip(self, _req), level = "debug")]
    async fn getattr(
        &self,
        _req: Request,
        inode: Inode,
        _fh: Option<u64>,
        _flags: u32,
    ) -> FuseResult<ReplyAttr> {
        self.metrics.inc_getattr_ops();

        // Check cache first
        if let Some(attr) = self.metadata_cache.get_attr(inode) {
            return Ok(ReplyAttr {
                ttl: self.attr_ttl,
                attr: attr.to_fuse3(),
            });
        }

        // Fetch from OBS
        let path = self.get_path(inode)?;
        let obs_path = self.obs_path(&path);

        let meta = self.obs_client.stat(&obs_path).await.map_err(|e| {
            if matches!(e, ObsFuseError::Storage(ref se) if se.kind() == opendal::ErrorKind::NotFound) {
                // Try as directory
                return Errno::from(libc::ENOENT);
            }
            error!(error = %e, "Failed to get attributes");
            Errno::from(libc::EIO)
        })?;

        let attr = self.attr_builder.from_object_meta(inode, &meta);
        self.metadata_cache.put_attr(inode, attr.clone());

        Ok(ReplyAttr {
            ttl: self.attr_ttl,
            attr: attr.to_fuse3(),
        })
    }

    /// Set file attributes
    #[instrument(skip(self, _req), level = "debug")]
    async fn setattr(
        &self,
        _req: Request,
        inode: Inode,
        _fh: Option<u64>,
        set_attr: SetAttr,
    ) -> FuseResult<ReplyAttr> {
        let path = self.get_path(inode)?;

        // Handle truncate
        if let Some(size) = set_attr.size {
            let obs_path = self.obs_path(&path);
            self.write_buffer.truncate(inode, &obs_path, size).await.map_err(|e| {
                error!(error = %e, "Failed to truncate");
                Errno::from(libc::EIO)
            })?;

            // Invalidate caches
            self.data_cache.invalidate(inode);
            self.readahead.invalidate(inode);
        }

        // Update cached attributes
        let attr = if let Some(mut attr) = self.metadata_cache.get_attr(inode) {
            // Apply mode if provided
            if let Some(mode) = set_attr.mode {
                attr.perm = (mode & 0o7777) as u16;
            }
            // Apply uid/gid if provided
            if let Some(uid) = set_attr.uid {
                attr.uid = uid;
            }
            if let Some(gid) = set_attr.gid {
                attr.gid = gid;
            }
            // Apply size if provided
            if let Some(size) = set_attr.size {
                attr.size = size;
                attr.blocks = (size + 511) / 512;
            }
            // Apply atime if provided
            if let Some(atime) = set_attr.atime {
                attr.atime = timestamp_to_systemtime(atime);
            }
            // Apply mtime if provided
            if let Some(mtime) = set_attr.mtime {
                attr.mtime = timestamp_to_systemtime(mtime);
            }

            self.metadata_cache.put_attr(inode, attr.clone());
            attr
        } else {
            // Create new attributes
            let entry = self.inode_mgr.get_entry(inode).ok_or(Errno::from(libc::ENOENT))?;
            let mut attr = entry.attr;

            if let Some(mode) = set_attr.mode {
                attr.perm = (mode & 0o7777) as u16;
            }
            if let Some(uid) = set_attr.uid {
                attr.uid = uid;
            }
            if let Some(gid) = set_attr.gid {
                attr.gid = gid;
            }
            if let Some(size) = set_attr.size {
                attr.size = size;
                attr.blocks = (size + 511) / 512;
            }

            self.metadata_cache.put_attr(inode, attr.clone());
            attr
        };

        Ok(ReplyAttr {
            ttl: self.attr_ttl,
            attr: attr.to_fuse3(),
        })
    }

    /// Open a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn open(&self, _req: Request, inode: Inode, flags: u32) -> FuseResult<ReplyOpen> {
        let fh = self.handle_mgr.open(inode, flags, false);

        // Handle O_TRUNC
        if flags & libc::O_TRUNC as u32 != 0 {
            let path = self.get_path(inode)?;
            let obs_path = self.obs_path(&path);
            self.write_buffer.truncate(inode, &obs_path, 0).await.map_err(|e| {
                error!(error = %e, "Failed to truncate on open");
                Errno::from(libc::EIO)
            })?;
            self.data_cache.invalidate(inode);
        }

        Ok(ReplyOpen { fh, flags: 0 })
    }

    /// Read from a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn read(
        &self,
        _req: Request,
        inode: Inode,
        fh: u64,
        offset: u64,
        size: u32,
    ) -> FuseResult<ReplyData> {
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
            return Ok(ReplyData {
                data: data.slice(0..end),
            });
        }

        // Check data cache
        if let Some(data) = self.data_cache.get(inode, offset) {
            let start = (offset % self.data_cache.block_size()) as usize;
            let end = (start + size as usize).min(data.len());
            if start < data.len() {
                return Ok(ReplyData {
                    data: data.slice(start..end),
                });
            }
        }

        // Fetch from OBS
        let data = self
            .obs_client
            .read_range(&obs_path, offset, size as u64)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to read");
                Errno::from(libc::EIO)
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

        Ok(ReplyData { data })
    }

    /// Write to a file
    #[instrument(skip(self, _req, data), level = "debug")]
    async fn write(
        &self,
        _req: Request,
        inode: Inode,
        fh: u64,
        offset: u64,
        data: &[u8],
        _write_flags: u32,
        _flags: u32,
    ) -> FuseResult<ReplyWrite> {
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
                Errno::from(libc::EIO)
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

        Ok(ReplyWrite {
            written: written as u32,
        })
    }

    /// Release (close) a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn release(
        &self,
        _req: Request,
        inode: Inode,
        fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
    ) -> FuseResult<()> {
        // Flush write buffer
        self.write_buffer.release(inode).await.map_err(|e| {
            error!(error = %e, "Failed to flush on release");
            Errno::from(libc::EIO)
        })?;

        // Close handle
        self.handle_mgr.close(fh);

        // Invalidate metadata cache to ensure consistency
        self.metadata_cache.invalidate_attr(inode);

        Ok(())
    }

    /// Synchronize file contents
    #[instrument(skip(self, _req), level = "debug")]
    async fn fsync(&self, _req: Request, inode: Inode, _fh: u64, _datasync: bool) -> FuseResult<()> {
        self.write_buffer.sync_flush(inode).await.map_err(|e| {
            error!(error = %e, "Failed to fsync");
            Errno::from(libc::EIO)
        })?;

        Ok(())
    }

    /// Open a directory
    #[instrument(skip(self, _req), level = "debug")]
    async fn opendir(&self, _req: Request, inode: Inode, flags: u32) -> FuseResult<ReplyOpen> {
        let fh = self.handle_mgr.open(inode, flags, true);
        Ok(ReplyOpen { fh, flags: 0 })
    }

    /// Read directory entries
    #[instrument(skip(self, _req), level = "debug")]
    async fn readdir(
        &self,
        _req: Request,
        inode: Inode,
        _fh: u64,
        offset: i64,
    ) -> FuseResult<ReplyDirectory<Self::DirEntryStream<'_>>> {
        self.metrics.inc_readdir_ops();

        let entries = self.do_readdir(inode, offset).await?;

        let dir_entries: Vec<FuseResult<DirectoryEntry>> = entries
            .into_iter()
            .map(|e| {
                Ok(DirectoryEntry {
                    inode: e.inode,
                    kind: e.kind,
                    name: e.name,
                    offset: e.offset,
                })
            })
            .collect();

        Ok(ReplyDirectory {
            entries: futures::stream::iter(dir_entries),
        })
    }

    /// Release (close) a directory
    #[instrument(skip(self, _req), level = "debug")]
    async fn releasedir(&self, _req: Request, _inode: Inode, fh: u64, _flags: u32) -> FuseResult<()> {
        self.handle_mgr.close(fh);
        Ok(())
    }

    /// Create a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn create(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
        mode: u32,
        flags: u32,
    ) -> FuseResult<ReplyCreated> {
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
                Errno::from(libc::EIO)
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

        Ok(ReplyCreated {
            ttl: self.entry_ttl,
            attr: attr.to_fuse3(),
            generation: 0,
            fh,
            flags: 0,
        })
    }

    /// Create a directory
    #[instrument(skip(self, _req), level = "debug")]
    async fn mkdir(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
        mode: u32,
        _umask: u32,
    ) -> FuseResult<ReplyEntry> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Create directory marker in OBS
        self.obs_client.create_dir(&obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to create directory");
            Errno::from(libc::EIO)
        })?;

        // Create inode
        let inode = self.inode_mgr.get_or_create_inode(&child_path, true, 0);
        let attr = self.attr_builder.new_directory(inode, Some(mode));
        self.metadata_cache.put_attr(inode, attr.clone());

        // Remove from negative cache
        self.metadata_cache.remove_negative(&child_path);

        // Invalidate parent directory cache
        self.metadata_cache.invalidate_dir(parent);

        Ok(ReplyEntry {
            ttl: self.entry_ttl,
            attr: attr.to_fuse3(),
            generation: 0,
        })
    }

    /// Remove a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn unlink(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<()> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Delete from OBS
        self.obs_client.delete(&obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to delete file");
            Errno::from(libc::EIO)
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
    #[instrument(skip(self, _req), level = "debug")]
    async fn rmdir(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<()> {
        let parent_path = self.get_path(parent)?;
        let name_str = name.to_string_lossy();
        let child_path = InodeManager::join_path(&parent_path, &name_str);
        let obs_path = self.obs_path(&child_path);

        // Check if directory is empty
        let list_prefix = format!("{}/", obs_path.trim_end_matches('/'));
        let entries = self.obs_client.list(&list_prefix).await.map_err(|e| {
            error!(error = %e, "Failed to list directory");
            Errno::from(libc::EIO)
        })?;

        // Filter out the directory marker itself
        let non_marker_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.path != list_prefix && !e.path.ends_with("/."))
            .collect();

        if !non_marker_entries.is_empty() {
            return Err(Errno::from(libc::ENOTEMPTY).into());
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
    #[instrument(skip(self, _req), level = "debug")]
    async fn rename(
        &self,
        _req: Request,
        parent: Inode,
        name: &OsStr,
        new_parent: Inode,
        new_name: &OsStr,
    ) -> FuseResult<()> {
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
                Errno::from(libc::EIO)
            })?;

        self.obs_client.delete(&old_obs_path).await.map_err(|e| {
            error!(error = %e, "Failed to delete after rename");
            Errno::from(libc::EIO)
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

    /// Get filesystem statistics
    #[instrument(skip(self, _req), level = "debug")]
    async fn statfs(&self, _req: Request, _inode: Inode) -> FuseResult<ReplyStatFs> {
        // Return reasonable defaults for object storage
        Ok(ReplyStatFs {
            blocks: 1024 * 1024 * 1024, // 1PB in 1KB blocks
            bfree: 1024 * 1024 * 1024,
            bavail: 1024 * 1024 * 1024,
            files: 1_000_000_000,
            ffree: 1_000_000_000,
            bsize: 4096,
            namelen: 1024,
            frsize: 4096,
        })
    }

    /// Flush file data
    #[instrument(skip(self, _req), level = "debug")]
    async fn flush(&self, _req: Request, inode: Inode, _fh: u64, _lock_owner: u64) -> FuseResult<()> {
        self.write_buffer.flush(inode).await.map_err(|e| {
            error!(error = %e, "Failed to flush");
            Errno::from(libc::EIO)
        })?;

        Ok(())
    }

    /// Check file access permissions
    #[instrument(skip(self, _req), level = "debug")]
    async fn access(&self, _req: Request, inode: Inode, _mask: u32) -> FuseResult<()> {
        // For now, allow all access
        // A full implementation would check against the permission config
        if self.inode_mgr.get_entry(inode).is_some() {
            Ok(())
        } else {
            Err(Errno::from(libc::ENOENT).into())
        }
    }
}
