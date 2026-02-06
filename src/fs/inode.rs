//! Inode management for OBS FUSE filesystem
//!
//! This module provides stable inode numbers for OBS objects,
//! mapping between paths and inodes.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};
use tracing::debug;

use crate::config::PermissionConfig;
use crate::fs::platform::{current_gid, current_uid, FileKind};

/// Root inode number (FUSE convention)
pub const ROOT_INODE: u64 = 1;

/// Inode manager for path-to-inode mapping
pub struct InodeManager {
    /// Path to inode mapping
    path_to_inode: DashMap<String, u64>,
    /// Inode to entry mapping
    inode_to_entry: DashMap<u64, InodeEntry>,
    /// Next inode number
    next_inode: AtomicU64,
    /// Permission configuration
    permission_config: PermissionConfig,
}

/// Entry stored for each inode
#[derive(Debug, Clone)]
pub struct InodeEntry {
    /// Object path in OBS
    pub path: String,
    /// File attributes
    pub attr: FileAttr,
    /// Whether this is a directory
    pub is_dir: bool,
    /// Cached children inodes (for directories)
    pub children: Option<Vec<u64>>,
    /// When this entry was cached
    pub cached_at: Instant,
    /// Reference count (for open handles)
    pub ref_count: u64,
}

/// File attributes (platform-agnostic)
#[derive(Debug, Clone)]
pub struct FileAttr {
    /// Inode number
    pub ino: u64,
    /// Size in bytes
    pub size: u64,
    /// Number of blocks
    pub blocks: u64,
    /// Access time
    pub atime: SystemTime,
    /// Modification time
    pub mtime: SystemTime,
    /// Change time
    pub ctime: SystemTime,
    /// Creation time
    pub crtime: SystemTime,
    /// File type
    pub kind: FileKind,
    /// Permission mode
    pub perm: u16,
    /// Number of hard links
    pub nlink: u32,
    /// User ID
    pub uid: u32,
    /// Group ID
    pub gid: u32,
    /// Device ID (for special files)
    pub rdev: u32,
    /// Block size
    pub blksize: u32,
    /// Flags (macOS only)
    pub flags: u32,
}

impl Default for FileAttr {
    fn default() -> Self {
        let now = SystemTime::now();
        Self {
            ino: 0,
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileKind::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: current_uid(),
            gid: current_gid(),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
}

impl FileAttr {
    /// Create attributes for a directory
    pub fn directory(ino: u64, uid: u32, gid: u32, mode: u32) -> Self {
        let now = SystemTime::now();
        Self {
            ino,
            size: 4096,
            blocks: 8,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileKind::Directory,
            perm: (mode & 0o7777) as u16,
            nlink: 2,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Create attributes for a regular file
    pub fn file(ino: u64, size: u64, uid: u32, gid: u32, mode: u32) -> Self {
        let now = SystemTime::now();
        let blocks = (size + 511) / 512;
        Self {
            ino,
            size,
            blocks,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileKind::RegularFile,
            perm: (mode & 0o7777) as u16,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Create attributes for a symlink
    pub fn symlink(ino: u64, size: u64, uid: u32, gid: u32) -> Self {
        let now = SystemTime::now();
        Self {
            ino,
            size,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileKind::Symlink,
            perm: 0o777,
            nlink: 1,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Convert to fuse3 FileAttr (Unix only)
    #[cfg(unix)]
    pub fn to_fuse3(&self) -> fuse3::raw::prelude::FileAttr {
        fuse3::raw::prelude::FileAttr {
            ino: self.ino,
            size: self.size,
            blocks: self.blocks,
            atime: systemtime_to_fuse_timestamp(self.atime),
            mtime: systemtime_to_fuse_timestamp(self.mtime),
            ctime: systemtime_to_fuse_timestamp(self.ctime),
            kind: self.kind.into(),
            perm: self.perm,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
            rdev: self.rdev,
            blksize: self.blksize,
            #[cfg(target_os = "macos")]
            crtime: systemtime_to_fuse_timestamp(self.crtime),
            #[cfg(target_os = "macos")]
            flags: self.flags,
        }
    }

    /// Convert to WinFSP FileInfo (Windows only)
    #[cfg(windows)]
    pub fn to_winfsp(&self) -> winfsp::FileInfo {
        use crate::fs::platform::Timestamp;

        let creation_time = Timestamp::from(self.crtime).to_filetime();
        let last_access_time = Timestamp::from(self.atime).to_filetime();
        let last_write_time = Timestamp::from(self.mtime).to_filetime();
        let change_time = Timestamp::from(self.ctime).to_filetime();

        winfsp::FileInfo {
            file_attributes: self.kind.to_win_attrs(),
            reparse_tag: 0,
            allocation_size: self.blocks * 512,
            file_size: self.size,
            creation_time,
            last_access_time,
            last_write_time,
            change_time,
            index_number: self.ino,
            hard_links: self.nlink,
            ea_size: 0,
        }
    }
}

/// Convert SystemTime to fuse3 Timestamp (Unix only)
#[cfg(unix)]
fn systemtime_to_fuse_timestamp(st: SystemTime) -> fuse3::Timestamp {
    match st.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => fuse3::Timestamp {
            sec: duration.as_secs() as i64,
            nsec: duration.subsec_nanos(),
        },
        Err(_) => fuse3::Timestamp { sec: 0, nsec: 0 },
    }
}

impl InodeManager {
    /// Create a new inode manager
    pub fn new(permission_config: PermissionConfig) -> Self {
        let manager = Self {
            path_to_inode: DashMap::new(),
            inode_to_entry: DashMap::new(),
            next_inode: AtomicU64::new(ROOT_INODE + 1),
            permission_config,
        };

        // Initialize root inode
        manager.init_root();

        manager
    }

    /// Initialize root directory
    fn init_root(&self) {
        let attr = FileAttr::directory(
            ROOT_INODE,
            self.permission_config.fixed.uid,
            self.permission_config.fixed.gid,
            self.permission_config.fixed.dir_mode,
        );

        let entry = InodeEntry {
            path: String::new(),
            attr,
            is_dir: true,
            children: None,
            cached_at: Instant::now(),
            ref_count: 1,
        };

        self.path_to_inode.insert(String::new(), ROOT_INODE);
        self.inode_to_entry.insert(ROOT_INODE, entry);
    }

    /// Get or create inode for a path
    pub fn get_or_create_inode(&self, path: &str, is_dir: bool, size: u64) -> u64 {
        let normalized_path = Self::normalize_path(path);

        // Check if already exists
        if let Some(inode) = self.path_to_inode.get(&normalized_path) {
            return *inode;
        }

        // Create new inode
        let inode = self.next_inode.fetch_add(1, Ordering::SeqCst);

        let attr = if is_dir {
            FileAttr::directory(
                inode,
                self.permission_config.fixed.uid,
                self.permission_config.fixed.gid,
                self.permission_config.fixed.dir_mode,
            )
        } else {
            FileAttr::file(
                inode,
                size,
                self.permission_config.fixed.uid,
                self.permission_config.fixed.gid,
                self.permission_config.fixed.file_mode,
            )
        };

        let entry = InodeEntry {
            path: normalized_path.clone(),
            attr,
            is_dir,
            children: None,
            cached_at: Instant::now(),
            ref_count: 0,
        };

        self.path_to_inode.insert(normalized_path, inode);
        self.inode_to_entry.insert(inode, entry);

        debug!(inode = inode, path = path, is_dir = is_dir, "Created inode");

        inode
    }

    /// Get inode for a path
    pub fn get_inode(&self, path: &str) -> Option<u64> {
        let normalized_path = Self::normalize_path(path);
        self.path_to_inode.get(&normalized_path).map(|v| *v)
    }

    /// Get entry by inode
    pub fn get_entry(&self, inode: u64) -> Option<InodeEntry> {
        self.inode_to_entry.get(&inode).map(|v| v.clone())
    }

    /// Get path by inode
    pub fn get_path(&self, inode: u64) -> Option<String> {
        self.inode_to_entry.get(&inode).map(|v| v.path.clone())
    }

    /// Update entry attributes
    pub fn update_attr<F>(&self, inode: u64, f: F)
    where
        F: FnOnce(&mut FileAttr),
    {
        if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
            f(&mut entry.attr);
            entry.cached_at = Instant::now();
        }
    }

    /// Update entry size
    pub fn update_size(&self, inode: u64, size: u64) {
        self.update_attr(inode, |attr| {
            attr.size = size;
            attr.blocks = (size + 511) / 512;
            attr.mtime = SystemTime::now();
        });
    }

    /// Update entry mtime
    pub fn update_mtime(&self, inode: u64, mtime: SystemTime) {
        self.update_attr(inode, |attr| {
            attr.mtime = mtime;
        });
    }

    /// Set children for a directory
    pub fn set_children(&self, inode: u64, children: Vec<u64>) {
        if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
            entry.children = Some(children);
            entry.cached_at = Instant::now();
        }
    }

    /// Get children of a directory
    pub fn get_children(&self, inode: u64) -> Option<Vec<u64>> {
        self.inode_to_entry
            .get(&inode)
            .and_then(|e| e.children.clone())
    }

    /// Increment reference count
    pub fn inc_ref(&self, inode: u64) {
        if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
            entry.ref_count += 1;
        }
    }

    /// Decrement reference count
    pub fn dec_ref(&self, inode: u64) -> u64 {
        if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            entry.ref_count
        } else {
            0
        }
    }

    /// Remove inode
    pub fn remove(&self, inode: u64) {
        if let Some((_, entry)) = self.inode_to_entry.remove(&inode) {
            self.path_to_inode.remove(&entry.path);
            debug!(inode = inode, path = %entry.path, "Removed inode");
        }
    }

    /// Remove inode by path
    pub fn remove_by_path(&self, path: &str) {
        let normalized_path = Self::normalize_path(path);
        if let Some((_, inode)) = self.path_to_inode.remove(&normalized_path) {
            self.inode_to_entry.remove(&inode);
            debug!(inode = inode, path = path, "Removed inode by path");
        }
    }

    /// Rename/move inode
    pub fn rename(&self, old_path: &str, new_path: &str) {
        let old_normalized = Self::normalize_path(old_path);
        let new_normalized = Self::normalize_path(new_path);

        if let Some((_, inode)) = self.path_to_inode.remove(&old_normalized) {
            self.path_to_inode.insert(new_normalized.clone(), inode);
            if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
                entry.path = new_normalized;
                entry.cached_at = Instant::now();
            }
        }
    }

    /// Check if entry is expired
    pub fn is_expired(&self, inode: u64, ttl: Duration) -> bool {
        self.inode_to_entry
            .get(&inode)
            .map(|e| e.cached_at.elapsed() > ttl)
            .unwrap_or(true)
    }

    /// Invalidate entry (mark as expired)
    pub fn invalidate(&self, inode: u64) {
        if let Some(mut entry) = self.inode_to_entry.get_mut(&inode) {
            entry.cached_at = Instant::now() - Duration::from_secs(3600);
            entry.children = None;
        }
    }

    /// Invalidate by path
    pub fn invalidate_path(&self, path: &str) {
        let normalized_path = Self::normalize_path(path);
        if let Some(inode) = self.path_to_inode.get(&normalized_path) {
            self.invalidate(*inode);
        }
    }

    /// Get parent path
    pub fn parent_path(path: &str) -> Option<String> {
        let normalized = Self::normalize_path(path);
        if normalized.is_empty() {
            return None;
        }

        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() <= 1 {
            Some(String::new())
        } else {
            Some(parts[..parts.len() - 1].join("/"))
        }
    }

    /// Get file name from path
    pub fn file_name(path: &str) -> &str {
        let path = path.trim_end_matches('/');
        path.rsplit('/').next().unwrap_or(path)
    }

    /// Join path components
    pub fn join_path(parent: &str, name: &str) -> String {
        if parent.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent.trim_end_matches('/'), name)
        }
    }

    /// Normalize path (remove leading/trailing slashes, handle empty)
    fn normalize_path(path: &str) -> String {
        let path = path.trim_matches('/');
        // Also handle Windows-style path separators
        #[cfg(windows)]
        let path = path.replace('\\', "/");
        #[cfg(windows)]
        return path.trim_matches('/').to_string();
        #[cfg(unix)]
        path.to_string()
    }

    /// Get statistics
    pub fn stats(&self) -> InodeStats {
        InodeStats {
            total_inodes: self.inode_to_entry.len(),
            next_inode: self.next_inode.load(Ordering::Relaxed),
        }
    }
}

/// Inode manager statistics
#[derive(Debug, Clone)]
pub struct InodeStats {
    pub total_inodes: usize,
    pub next_inode: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FixedPermission, PermissionMode};

    fn test_permission_config() -> PermissionConfig {
        PermissionConfig {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission {
                uid: 1000,
                gid: 1000,
                file_mode: 0o644,
                dir_mode: 0o755,
            },
        }
    }

    #[test]
    fn test_inode_manager_creation() {
        let manager = InodeManager::new(test_permission_config());

        // Root should exist
        assert!(manager.get_entry(ROOT_INODE).is_some());
        assert_eq!(manager.get_inode(""), Some(ROOT_INODE));
    }

    #[test]
    fn test_get_or_create_inode() {
        let manager = InodeManager::new(test_permission_config());

        let inode1 = manager.get_or_create_inode("test/file.txt", false, 100);
        let inode2 = manager.get_or_create_inode("test/file.txt", false, 100);

        assert_eq!(inode1, inode2);
        assert!(inode1 > ROOT_INODE);
    }

    #[test]
    fn test_path_operations() {
        assert_eq!(InodeManager::parent_path("a/b/c"), Some("a/b".to_string()));
        assert_eq!(InodeManager::parent_path("a"), Some(String::new()));
        assert_eq!(InodeManager::parent_path(""), None);

        assert_eq!(InodeManager::file_name("a/b/c.txt"), "c.txt");
        assert_eq!(InodeManager::file_name("file.txt"), "file.txt");

        assert_eq!(InodeManager::join_path("a/b", "c"), "a/b/c");
        assert_eq!(InodeManager::join_path("", "c"), "c");
    }

    #[test]
    fn test_rename() {
        let manager = InodeManager::new(test_permission_config());

        let inode = manager.get_or_create_inode("old/path.txt", false, 100);
        manager.rename("old/path.txt", "new/path.txt");

        assert_eq!(manager.get_inode("new/path.txt"), Some(inode));
        assert_eq!(manager.get_inode("old/path.txt"), None);
    }
}
