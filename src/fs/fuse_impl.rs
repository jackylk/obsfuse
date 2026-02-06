//! Unix FUSE implementation for OBS filesystem
//!
//! This module implements the fuse3 Filesystem trait, wrapping the
//! platform-independent ObsFsCore with FUSE-specific handling.

#![cfg(unix)]

use fuse3::raw::prelude::*;
use fuse3::{Errno, Inode, Result as FuseResult, SetAttr, Timestamp};
use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{error, info, instrument};

use crate::config::Config;
use crate::fs::core::ObsFsCore;
use crate::utils::{Metrics, ObsFuseError};

/// OBS FUSE filesystem (Unix implementation)
pub struct ObsFs {
    /// Core filesystem logic
    core: Arc<ObsFsCore>,
}

impl ObsFs {
    /// Create a new OBS filesystem
    pub fn new(config: Config, metrics: Arc<Metrics>) -> Result<Self, ObsFuseError> {
        let core = Arc::new(ObsFsCore::new(config, metrics)?);
        Ok(Self { core })
    }

    /// Get reference to the core
    pub fn core(&self) -> &Arc<ObsFsCore> {
        &self.core
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

/// Convert core FsError to fuse3 Errno
fn fs_err_to_errno(err: crate::fs::core::FsError) -> Errno {
    Errno::from(err.errno)
}

impl Filesystem for ObsFs {
    type DirEntryStream<'a> = futures::stream::Iter<std::vec::IntoIter<FuseResult<DirectoryEntry>>> where Self: 'a;
    type DirEntryPlusStream<'a> = futures::stream::Iter<std::vec::IntoIter<FuseResult<DirectoryEntryPlus>>> where Self: 'a;

    /// Initialize filesystem
    #[instrument(skip(self, _req), level = "debug")]
    async fn init(&self, _req: Request) -> FuseResult<ReplyInit> {
        info!("Initializing OBS FUSE filesystem");
        Ok(ReplyInit {
            max_write: NonZeroU32::new(self.core.config.fuse.max_write.as_u64() as u32).unwrap(),
        })
    }

    /// Clean up filesystem
    #[instrument(skip(self, _req), level = "debug")]
    async fn destroy(&self, _req: Request) {
        info!("Destroying OBS FUSE filesystem");

        // Flush all write buffers
        if let Err(e) = self.core.flush_all().await {
            error!(error = ?e, "Failed to flush write buffers on destroy");
        }
    }

    /// Look up a directory entry
    #[instrument(skip(self, _req), level = "debug")]
    async fn lookup(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<ReplyEntry> {
        self.core.metrics.inc_lookup_ops();

        let (_inode, attr) = self.core.do_lookup(parent, name).await.map_err(fs_err_to_errno)?;

        Ok(ReplyEntry {
            ttl: self.core.entry_ttl,
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
        let attr = self.core.do_getattr(inode).await.map_err(fs_err_to_errno)?;

        Ok(ReplyAttr {
            ttl: self.core.attr_ttl,
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
        // Handle truncate
        if let Some(size) = set_attr.size {
            self.core.do_truncate(inode, size).await.map_err(fs_err_to_errno)?;
        }

        // Update cached attributes
        let attr = if let Some(mut attr) = self.core.metadata_cache.get_attr(inode) {
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

            self.core.metadata_cache.put_attr(inode, attr.clone());
            attr
        } else {
            // Create new attributes
            let entry = self.core.inode_mgr.get_entry(inode).ok_or(Errno::from(libc::ENOENT))?;
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

            self.core.metadata_cache.put_attr(inode, attr.clone());
            attr
        };

        Ok(ReplyAttr {
            ttl: self.core.attr_ttl,
            attr: attr.to_fuse3(),
        })
    }

    /// Open a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn open(&self, _req: Request, inode: Inode, flags: u32) -> FuseResult<ReplyOpen> {
        let fh = self.core.do_open(inode, flags);

        // Handle O_TRUNC
        if flags & libc::O_TRUNC as u32 != 0 {
            self.core.do_truncate(inode, 0).await.map_err(fs_err_to_errno)?;
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
        let data = self.core.do_read(inode, fh, offset, size).await.map_err(fs_err_to_errno)?;
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
        let written = self.core.do_write(inode, fh, offset, data).await.map_err(fs_err_to_errno)?;
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
        self.core.do_release(inode, fh).await.map_err(fs_err_to_errno)?;
        Ok(())
    }

    /// Synchronize file contents
    #[instrument(skip(self, _req), level = "debug")]
    async fn fsync(&self, _req: Request, inode: Inode, _fh: u64, _datasync: bool) -> FuseResult<()> {
        self.core.do_fsync(inode).await.map_err(fs_err_to_errno)?;
        Ok(())
    }

    /// Open a directory
    #[instrument(skip(self, _req), level = "debug")]
    async fn opendir(&self, _req: Request, inode: Inode, flags: u32) -> FuseResult<ReplyOpen> {
        let fh = self.core.do_opendir(inode, flags);
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
        self.core.metrics.inc_readdir_ops();

        let entries = self.core.do_readdir(inode, offset).await.map_err(fs_err_to_errno)?;

        let dir_entries: Vec<FuseResult<DirectoryEntry>> = entries
            .into_iter()
            .map(|e| {
                Ok(DirectoryEntry {
                    inode: e.inode,
                    kind: e.kind.into(),
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
        self.core.do_close(fh);
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
        let (_inode, attr, fh) = self.core.do_create(parent, name, mode, flags).await.map_err(fs_err_to_errno)?;

        Ok(ReplyCreated {
            ttl: self.core.entry_ttl,
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
        let (_inode, attr) = self.core.do_mkdir(parent, name, mode).await.map_err(fs_err_to_errno)?;

        Ok(ReplyEntry {
            ttl: self.core.entry_ttl,
            attr: attr.to_fuse3(),
            generation: 0,
        })
    }

    /// Remove a file
    #[instrument(skip(self, _req), level = "debug")]
    async fn unlink(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<()> {
        self.core.do_unlink(parent, name).await.map_err(fs_err_to_errno)?;
        Ok(())
    }

    /// Remove a directory
    #[instrument(skip(self, _req), level = "debug")]
    async fn rmdir(&self, _req: Request, parent: Inode, name: &OsStr) -> FuseResult<()> {
        self.core.do_rmdir(parent, name).await.map_err(fs_err_to_errno)?;
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
        self.core.do_rename(parent, name, new_parent, new_name).await.map_err(fs_err_to_errno)?;
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
        self.core.do_flush(inode).await.map_err(fs_err_to_errno)?;
        Ok(())
    }

    /// Check file access permissions
    #[instrument(skip(self, _req), level = "debug")]
    async fn access(&self, _req: Request, inode: Inode, _mask: u32) -> FuseResult<()> {
        self.core.do_access(inode).map_err(fs_err_to_errno)?;
        Ok(())
    }
}
