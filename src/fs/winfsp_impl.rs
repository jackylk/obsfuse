//! Windows WinFSP implementation for OBS filesystem
//!
//! This module implements the WinFSP FileSystemInterface trait, wrapping the
//! platform-independent ObsFsCore with WinFSP-specific handling.

#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info};
use winfsp::filesystem::{
    DirInfo, FileInfo, FileSystemContext, FileSystemHost, FileSystemInterface,
    IoResult, OpenFileInfo, VolumeInfo,
};

use crate::config::Config;
use crate::fs::core::ObsFsCore;
use crate::fs::platform::{errno, FileKind, Timestamp};
use crate::utils::{Metrics, ObsFuseError};

/// File context for WinFSP handles
#[derive(Debug)]
pub struct WinFspFileContext {
    /// Associated inode
    pub inode: u64,
    /// File handle from core
    pub fh: u64,
    /// Whether this is a directory
    pub is_dir: bool,
}

/// OBS WinFSP filesystem adapter
pub struct WinFspFs {
    /// Core filesystem logic
    core: Arc<ObsFsCore>,
    /// Tokio runtime handle for async operations
    runtime: tokio::runtime::Handle,
}

impl WinFspFs {
    /// Create a new WinFSP filesystem adapter
    pub fn new(config: Config, metrics: Arc<Metrics>) -> Result<Self, ObsFuseError> {
        let core = Arc::new(ObsFsCore::new(config, metrics)?);
        let runtime = tokio::runtime::Handle::current();
        Ok(Self { core, runtime })
    }

    /// Get reference to the core
    pub fn core(&self) -> &Arc<ObsFsCore> {
        &self.core
    }

    /// Run an async operation synchronously
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    /// Convert FsError to WinFSP status
    fn fs_err_to_status(err: crate::fs::core::FsError) -> winfsp::FspError {
        use windows::Win32::Foundation::{
            STATUS_ACCESS_DENIED, STATUS_DIRECTORY_NOT_EMPTY, STATUS_FILE_IS_A_DIRECTORY,
            STATUS_INVALID_PARAMETER, STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_NOT_FOUND,
            STATUS_UNSUCCESSFUL,
        };

        match err.errno {
            e if e == errno::ENOENT => winfsp::FspError::from(STATUS_OBJECT_NAME_NOT_FOUND),
            e if e == errno::ENOTEMPTY => winfsp::FspError::from(STATUS_DIRECTORY_NOT_EMPTY),
            e if e == errno::EACCES => winfsp::FspError::from(STATUS_ACCESS_DENIED),
            e if e == errno::EISDIR => winfsp::FspError::from(STATUS_FILE_IS_A_DIRECTORY),
            e if e == errno::ENOTDIR => winfsp::FspError::from(STATUS_NOT_A_DIRECTORY),
            e if e == errno::EINVAL => winfsp::FspError::from(STATUS_INVALID_PARAMETER),
            _ => winfsp::FspError::from(STATUS_UNSUCCESSFUL),
        }
    }

    /// Build FileInfo from our FileAttr
    fn build_file_info(attr: &crate::fs::inode::FileAttr) -> FileInfo {
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        };

        let file_attributes = match attr.kind {
            FileKind::Directory => FILE_ATTRIBUTE_DIRECTORY.0,
            _ => FILE_ATTRIBUTE_NORMAL.0,
        };

        let creation_time = Timestamp::from(attr.crtime).to_filetime();
        let last_access_time = Timestamp::from(attr.atime).to_filetime();
        let last_write_time = Timestamp::from(attr.mtime).to_filetime();
        let change_time = Timestamp::from(attr.ctime).to_filetime();

        FileInfo {
            file_attributes,
            reparse_tag: 0,
            allocation_size: attr.blocks * 512,
            file_size: attr.size,
            creation_time,
            last_access_time,
            last_write_time,
            change_time,
            index_number: attr.ino,
            hard_links: 0,
            ea_size: 0,
        }
    }

    /// Normalize Windows path to Unix style
    fn normalize_path(path: &OsStr) -> String {
        let path_str = path.to_string_lossy();
        // Remove leading backslash and convert to forward slashes
        path_str.trim_start_matches('\\').replace('\\', "/")
    }
}

impl FileSystemInterface for WinFspFs {
    type FileContext = WinFspFileContext;

    fn get_volume_info(&self) -> IoResult<VolumeInfo> {
        Ok(VolumeInfo {
            total_size: 1024 * 1024 * 1024 * 1024, // 1TB
            free_size: 1024 * 1024 * 1024 * 1024,  // 1TB
            volume_label: "OBS".into(),
        })
    }

    fn set_volume_label(&self, _volume_label: &OsStr) -> IoResult<()> {
        // Read-only operation for now
        Ok(())
    }

    fn get_security_by_name(
        &self,
        file_name: &OsStr,
        _security_descriptor: Option<&mut [u8]>,
        _resolve_reparse_points: bool,
    ) -> IoResult<(u32, u64)> {
        let path = Self::normalize_path(file_name);

        // Get the inode for this path
        let inode = if path.is_empty() {
            crate::fs::inode::ROOT_INODE
        } else {
            // Parse path to find inode
            self.block_on(async {
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                let mut current_inode = crate::fs::inode::ROOT_INODE;

                for part in parts {
                    let name = OsStr::new(part);
                    match self.core.do_lookup(current_inode, name).await {
                        Ok((inode, _)) => current_inode = inode,
                        Err(_) => return Err(winfsp::FspError::from(
                            windows::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND,
                        )),
                    }
                }
                Ok(current_inode)
            })?
        };

        // Get attributes
        let attr = self.block_on(self.core.do_getattr(inode))
            .map_err(Self::fs_err_to_status)?;

        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        };

        let file_attributes = match attr.kind {
            FileKind::Directory => FILE_ATTRIBUTE_DIRECTORY.0,
            _ => FILE_ATTRIBUTE_NORMAL.0,
        };

        Ok((file_attributes, 0))
    }

    fn open(
        &self,
        file_name: &OsStr,
        create_options: u32,
        granted_access: u32,
    ) -> IoResult<OpenFileInfo<Self::FileContext>> {
        let path = Self::normalize_path(file_name);
        debug!(path = %path, "WinFSP open");

        // Get the inode for this path
        let inode = if path.is_empty() {
            crate::fs::inode::ROOT_INODE
        } else {
            // Parse path to find inode
            self.block_on(async {
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                let mut current_inode = crate::fs::inode::ROOT_INODE;

                for part in parts {
                    let name = OsStr::new(part);
                    match self.core.do_lookup(current_inode, name).await {
                        Ok((inode, _)) => current_inode = inode,
                        Err(e) => return Err(Self::fs_err_to_status(e)),
                    }
                }
                Ok(current_inode)
            })?
        };

        let attr = self.block_on(self.core.do_getattr(inode))
            .map_err(Self::fs_err_to_status)?;

        let is_dir = attr.kind == FileKind::Directory;

        // Convert granted_access to Unix-style flags
        let flags = if granted_access & 0x80000000 != 0 {
            // GENERIC_READ
            crate::fs::platform::open_flags::O_RDONLY
        } else if granted_access & 0x40000000 != 0 {
            // GENERIC_WRITE
            crate::fs::platform::open_flags::O_WRONLY
        } else {
            crate::fs::platform::open_flags::O_RDWR
        };

        let fh = if is_dir {
            self.core.do_opendir(inode, flags)
        } else {
            self.core.do_open(inode, flags)
        };

        let context = WinFspFileContext { inode, fh, is_dir };
        let file_info = Self::build_file_info(&attr);

        Ok(OpenFileInfo {
            context,
            file_info,
        })
    }

    fn close(&self, context: Self::FileContext) {
        debug!(inode = context.inode, "WinFSP close");
        if context.is_dir {
            self.core.do_close(context.fh);
        } else {
            let _ = self.block_on(self.core.do_release(context.inode, context.fh));
        }
    }

    fn read(
        &self,
        context: &Self::FileContext,
        buffer: &mut [u8],
        offset: u64,
    ) -> IoResult<u32> {
        debug!(inode = context.inode, offset = offset, len = buffer.len(), "WinFSP read");

        let data = self.block_on(self.core.do_read(
            context.inode,
            context.fh,
            offset,
            buffer.len() as u32,
        )).map_err(Self::fs_err_to_status)?;

        let bytes_read = data.len().min(buffer.len());
        buffer[..bytes_read].copy_from_slice(&data[..bytes_read]);

        Ok(bytes_read as u32)
    }

    fn write(
        &self,
        context: &Self::FileContext,
        buffer: &[u8],
        offset: u64,
        _write_to_end_of_file: bool,
        _constrained_io: bool,
    ) -> IoResult<(u32, FileInfo)> {
        debug!(inode = context.inode, offset = offset, len = buffer.len(), "WinFSP write");

        let written = self.block_on(self.core.do_write(
            context.inode,
            context.fh,
            offset,
            buffer,
        )).map_err(Self::fs_err_to_status)?;

        // Get updated file info
        let attr = self.block_on(self.core.do_getattr(context.inode))
            .map_err(Self::fs_err_to_status)?;

        Ok((written as u32, Self::build_file_info(&attr)))
    }

    fn flush(&self, context: &Self::FileContext) -> IoResult<FileInfo> {
        debug!(inode = context.inode, "WinFSP flush");

        self.block_on(self.core.do_flush(context.inode))
            .map_err(Self::fs_err_to_status)?;

        let attr = self.block_on(self.core.do_getattr(context.inode))
            .map_err(Self::fs_err_to_status)?;

        Ok(Self::build_file_info(&attr))
    }

    fn get_file_info(&self, context: &Self::FileContext) -> IoResult<FileInfo> {
        let attr = self.block_on(self.core.do_getattr(context.inode))
            .map_err(Self::fs_err_to_status)?;

        Ok(Self::build_file_info(&attr))
    }

    fn set_basic_info(
        &self,
        context: &Self::FileContext,
        file_attributes: u32,
        creation_time: u64,
        last_access_time: u64,
        last_write_time: u64,
        change_time: u64,
    ) -> IoResult<FileInfo> {
        // Update attributes in cache
        if let Some(mut attr) = self.core.metadata_cache.get_attr(context.inode) {
            if creation_time != 0 {
                attr.crtime = Timestamp::from_filetime(creation_time).into();
            }
            if last_access_time != 0 {
                attr.atime = Timestamp::from_filetime(last_access_time).into();
            }
            if last_write_time != 0 {
                attr.mtime = Timestamp::from_filetime(last_write_time).into();
            }
            if change_time != 0 {
                attr.ctime = Timestamp::from_filetime(change_time).into();
            }
            self.core.metadata_cache.put_attr(context.inode, attr.clone());
            Ok(Self::build_file_info(&attr))
        } else {
            let attr = self.block_on(self.core.do_getattr(context.inode))
                .map_err(Self::fs_err_to_status)?;
            Ok(Self::build_file_info(&attr))
        }
    }

    fn set_file_size(
        &self,
        context: &Self::FileContext,
        new_size: u64,
        set_allocation_size: bool,
    ) -> IoResult<FileInfo> {
        if !set_allocation_size {
            self.block_on(self.core.do_truncate(context.inode, new_size))
                .map_err(Self::fs_err_to_status)?;
        }

        let attr = self.block_on(self.core.do_getattr(context.inode))
            .map_err(Self::fs_err_to_status)?;

        Ok(Self::build_file_info(&attr))
    }

    fn can_delete(&self, context: &Self::FileContext, _file_name: &OsStr) -> IoResult<()> {
        // Check if directory is empty
        if context.is_dir {
            let entries = self.block_on(self.core.do_readdir(context.inode, 0))
                .map_err(Self::fs_err_to_status)?;

            // Filter out . and ..
            let real_entries: Vec<_> = entries
                .iter()
                .filter(|e| {
                    let name = e.name.to_string_lossy();
                    name != "." && name != ".."
                })
                .collect();

            if !real_entries.is_empty() {
                return Err(winfsp::FspError::from(
                    windows::Win32::Foundation::STATUS_DIRECTORY_NOT_EMPTY,
                ));
            }
        }

        Ok(())
    }

    fn rename(
        &self,
        context: &Self::FileContext,
        _file_name: &OsStr,
        new_file_name: &OsStr,
        _replace_if_exists: bool,
    ) -> IoResult<()> {
        let old_path = self.core.get_path(context.inode)
            .map_err(Self::fs_err_to_status)?;
        let new_path = Self::normalize_path(new_file_name);

        // Parse old path to get parent and name
        let (old_parent_path, old_name) = if let Some(pos) = old_path.rfind('/') {
            (&old_path[..pos], &old_path[pos + 1..])
        } else {
            ("", old_path.as_str())
        };

        // Parse new path to get parent and name
        let (new_parent_path, new_name) = if let Some(pos) = new_path.rfind('/') {
            (&new_path[..pos], &new_path[pos + 1..])
        } else {
            ("", new_path.as_str())
        };

        // Get parent inodes
        let old_parent = if old_parent_path.is_empty() {
            crate::fs::inode::ROOT_INODE
        } else {
            self.core.inode_mgr.get_inode(old_parent_path)
                .ok_or_else(|| winfsp::FspError::from(
                    windows::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND,
                ))?
        };

        let new_parent = if new_parent_path.is_empty() {
            crate::fs::inode::ROOT_INODE
        } else {
            self.core.inode_mgr.get_inode(new_parent_path)
                .ok_or_else(|| winfsp::FspError::from(
                    windows::Win32::Foundation::STATUS_OBJECT_NAME_NOT_FOUND,
                ))?
        };

        self.block_on(self.core.do_rename(
            old_parent,
            OsStr::new(old_name),
            new_parent,
            OsStr::new(new_name),
        )).map_err(Self::fs_err_to_status)?;

        Ok(())
    }

    fn get_security(&self, _context: &Self::FileContext) -> IoResult<Vec<u8>> {
        // Return empty security descriptor - we don't enforce Windows ACLs
        Ok(Vec::new())
    }

    fn set_security(&self, _context: &Self::FileContext, _security_descriptor: &[u8]) -> IoResult<()> {
        // Ignore security descriptor changes
        Ok(())
    }

    fn read_directory(
        &self,
        context: &Self::FileContext,
        marker: Option<&OsStr>,
        mut add_dir_info: impl FnMut(DirInfo) -> bool,
    ) -> IoResult<()> {
        debug!(inode = context.inode, "WinFSP read_directory");

        let entries = self.block_on(self.core.do_readdir(context.inode, 0))
            .map_err(Self::fs_err_to_status)?;

        let marker_str = marker.map(|m| m.to_string_lossy().to_string());
        let mut past_marker = marker_str.is_none();

        for entry in entries {
            let name = entry.name.to_string_lossy().to_string();

            // Skip entries until we're past the marker
            if !past_marker {
                if Some(&name) == marker_str.as_ref() {
                    past_marker = true;
                }
                continue;
            }

            let attr = self.block_on(self.core.do_getattr(entry.inode))
                .unwrap_or_else(|_| {
                    // Create default attributes if we can't get them
                    crate::fs::inode::FileAttr::default()
                });

            let dir_info = DirInfo {
                file_name: entry.name,
                file_info: Self::build_file_info(&attr),
            };

            if !add_dir_info(dir_info) {
                break;
            }
        }

        Ok(())
    }

    fn cleanup(&self, context: &Self::FileContext, _file_name: Option<&OsStr>, flags: u32) {
        const FspCleanupDelete: u32 = 0x01;

        if flags & FspCleanupDelete != 0 {
            // File/directory was marked for deletion
            let path = match self.core.get_path(context.inode) {
                Ok(p) => p,
                Err(_) => return,
            };

            // Get parent and name
            let (parent_path, name) = if let Some(pos) = path.rfind('/') {
                (&path[..pos], &path[pos + 1..])
            } else {
                ("", path.as_str())
            };

            let parent = if parent_path.is_empty() {
                crate::fs::inode::ROOT_INODE
            } else {
                match self.core.inode_mgr.get_inode(parent_path) {
                    Some(p) => p,
                    None => return,
                }
            };

            let result = if context.is_dir {
                self.block_on(self.core.do_rmdir(parent, OsStr::new(name)))
            } else {
                self.block_on(self.core.do_unlink(parent, OsStr::new(name)))
            };

            if let Err(e) = result {
                error!(error = ?e, "Failed to delete on cleanup");
            }
        }
    }

    fn create(
        &self,
        file_name: &OsStr,
        create_options: u32,
        granted_access: u32,
        file_attributes: u32,
        _security_descriptor: Option<&[u8]>,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        _extra_buffer_is_reparse_point: bool,
    ) -> IoResult<OpenFileInfo<Self::FileContext>> {
        let path = Self::normalize_path(file_name);
        debug!(path = %path, "WinFSP create");

        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

        let is_dir = (file_attributes & FILE_ATTRIBUTE_DIRECTORY.0) != 0
            || (create_options & 0x00000001) != 0; // FILE_DIRECTORY_FILE

        // Get parent and name
        let (parent_path, name) = if let Some(pos) = path.rfind('/') {
            (&path[..pos], &path[pos + 1..])
        } else {
            ("", path.as_str())
        };

        let parent = if parent_path.is_empty() {
            crate::fs::inode::ROOT_INODE
        } else {
            // Parse parent path
            self.block_on(async {
                let parts: Vec<&str> = parent_path.split('/').filter(|s| !s.is_empty()).collect();
                let mut current_inode = crate::fs::inode::ROOT_INODE;

                for part in parts {
                    let name = OsStr::new(part);
                    match self.core.do_lookup(current_inode, name).await {
                        Ok((inode, _)) => current_inode = inode,
                        Err(e) => return Err(Self::fs_err_to_status(e)),
                    }
                }
                Ok(current_inode)
            })?
        };

        // Convert granted_access to Unix-style flags
        let flags = if granted_access & 0x80000000 != 0 {
            crate::fs::platform::open_flags::O_RDONLY
        } else if granted_access & 0x40000000 != 0 {
            crate::fs::platform::open_flags::O_WRONLY
        } else {
            crate::fs::platform::open_flags::O_RDWR
        };

        let mode = 0o644; // Default mode

        let (inode, attr, fh) = if is_dir {
            let (inode, attr) = self.block_on(self.core.do_mkdir(parent, OsStr::new(name), 0o755))
                .map_err(Self::fs_err_to_status)?;
            let fh = self.core.do_opendir(inode, flags);
            (inode, attr, fh)
        } else {
            self.block_on(self.core.do_create(parent, OsStr::new(name), mode, flags))
                .map_err(Self::fs_err_to_status)?
        };

        let context = WinFspFileContext { inode, fh, is_dir };
        let file_info = Self::build_file_info(&attr);

        Ok(OpenFileInfo {
            context,
            file_info,
        })
    }

    fn overwrite(
        &self,
        context: &Self::FileContext,
        _file_attributes: u32,
        _replace_file_attributes: bool,
        _allocation_size: u64,
    ) -> IoResult<FileInfo> {
        // Truncate the file
        self.block_on(self.core.do_truncate(context.inode, 0))
            .map_err(Self::fs_err_to_status)?;

        let attr = self.block_on(self.core.do_getattr(context.inode))
            .map_err(Self::fs_err_to_status)?;

        Ok(Self::build_file_info(&attr))
    }
}

/// Mount the WinFSP filesystem
pub async fn mount_winfsp(
    config: Config,
    metrics: Arc<Metrics>,
    mountpoint: &str,
) -> Result<(), ObsFuseError> {
    info!(mountpoint = %mountpoint, "Mounting OBS filesystem via WinFSP");

    let fs = WinFspFs::new(config, metrics)?;

    // Create filesystem host
    let host = FileSystemHost::new(fs)
        .map_err(|e| ObsFuseError::Mount(format!("Failed to create filesystem host: {:?}", e)))?;

    // Mount the filesystem
    host.mount(mountpoint)
        .map_err(|e| ObsFuseError::Mount(format!("Failed to mount: {:?}", e)))?;

    info!("Filesystem mounted successfully");

    // Wait for stop signal
    tokio::signal::ctrl_c().await?;

    info!("Received interrupt signal, unmounting...");

    // Unmount
    host.unmount()
        .map_err(|e| ObsFuseError::Mount(format!("Failed to unmount: {:?}", e)))?;

    info!("Filesystem unmounted");

    Ok(())
}
