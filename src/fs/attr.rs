//! File attribute handling for OBS FUSE filesystem
//!
//! This module handles conversion between OBS metadata and FUSE file attributes.

use fuse3::FileType;
use std::time::SystemTime;

use crate::config::{PermissionConfig, PermissionMode};
use crate::fs::inode::FileAttr;
use crate::storage::ObjectMeta;

/// Attribute builder for creating file attributes from OBS metadata
pub struct AttrBuilder<'a> {
    permission_config: &'a PermissionConfig,
}

impl<'a> AttrBuilder<'a> {
    /// Create a new attribute builder
    pub fn new(permission_config: &'a PermissionConfig) -> Self {
        Self { permission_config }
    }

    /// Build file attributes from OBS object metadata
    pub fn from_object_meta(&self, inode: u64, meta: &ObjectMeta) -> FileAttr {
        let (uid, gid, mode) = self.get_permissions(meta);

        let mtime = meta.last_modified.unwrap_or_else(SystemTime::now);

        if meta.is_dir {
            let mut attr = FileAttr::directory(inode, uid, gid, mode);
            attr.mtime = mtime;
            attr.atime = mtime;
            attr.ctime = mtime;
            attr
        } else {
            let mut attr = FileAttr::file(inode, meta.size, uid, gid, mode);
            attr.mtime = mtime;
            attr.atime = mtime;
            attr.ctime = mtime;
            attr
        }
    }

    /// Get permissions based on configuration mode
    fn get_permissions(&self, meta: &ObjectMeta) -> (u32, u32, u32) {
        match self.permission_config.mode {
            PermissionMode::Fixed => {
                let mode = if meta.is_dir {
                    self.permission_config.fixed.dir_mode
                } else {
                    self.permission_config.fixed.file_mode
                };
                (
                    self.permission_config.fixed.uid,
                    self.permission_config.fixed.gid,
                    mode,
                )
            }
            PermissionMode::Preserved => {
                // In preserved mode, we would read from OBS metadata
                // For now, fall back to fixed permissions
                // TODO: Implement reading from x-obs-meta-uid, x-obs-meta-gid, x-obs-meta-mode
                let mode = if meta.is_dir {
                    self.permission_config.fixed.dir_mode
                } else {
                    self.permission_config.fixed.file_mode
                };
                (
                    self.permission_config.fixed.uid,
                    self.permission_config.fixed.gid,
                    mode,
                )
            }
        }
    }

    /// Create attributes for a new file
    pub fn new_file(&self, inode: u64, mode: Option<u32>) -> FileAttr {
        let file_mode = mode.unwrap_or(self.permission_config.fixed.file_mode);
        FileAttr::file(
            inode,
            0,
            self.permission_config.fixed.uid,
            self.permission_config.fixed.gid,
            file_mode,
        )
    }

    /// Create attributes for a new directory
    pub fn new_directory(&self, inode: u64, mode: Option<u32>) -> FileAttr {
        let dir_mode = mode.unwrap_or(self.permission_config.fixed.dir_mode);
        FileAttr::directory(
            inode,
            self.permission_config.fixed.uid,
            self.permission_config.fixed.gid,
            dir_mode,
        )
    }

    /// Create attributes for a symlink
    pub fn new_symlink(&self, inode: u64, target_len: u64) -> FileAttr {
        FileAttr::symlink(
            inode,
            target_len,
            self.permission_config.fixed.uid,
            self.permission_config.fixed.gid,
        )
    }
}

/// Setattr flags for partial attribute updates
#[derive(Debug, Clone, Default)]
pub struct SetAttrFlags {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub atime: Option<SystemTime>,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    pub fh: Option<u64>,
}

impl SetAttrFlags {
    /// Check if any attribute is being set
    pub fn has_any(&self) -> bool {
        self.mode.is_some()
            || self.uid.is_some()
            || self.gid.is_some()
            || self.size.is_some()
            || self.atime.is_some()
            || self.mtime.is_some()
            || self.ctime.is_some()
    }

    /// Check if size is being changed (truncate)
    pub fn is_truncate(&self) -> bool {
        self.size.is_some()
    }

    /// Apply flags to existing attributes
    pub fn apply(&self, attr: &mut FileAttr) {
        if let Some(mode) = self.mode {
            attr.perm = (mode & 0o7777) as u16;
        }
        if let Some(uid) = self.uid {
            attr.uid = uid;
        }
        if let Some(gid) = self.gid {
            attr.gid = gid;
        }
        if let Some(size) = self.size {
            attr.size = size;
            attr.blocks = (size + 511) / 512;
        }
        if let Some(atime) = self.atime {
            attr.atime = atime;
        }
        if let Some(mtime) = self.mtime {
            attr.mtime = mtime;
        }
        if let Some(ctime) = self.ctime {
            attr.ctime = ctime;
        }
    }
}

/// Determine file type from path
pub fn file_type_from_path(path: &str) -> FileType {
    if path.ends_with('/') || path.is_empty() {
        FileType::Directory
    } else {
        FileType::RegularFile
    }
}

/// Check if path represents a directory
pub fn is_directory_path(path: &str) -> bool {
    path.ends_with('/') || path.is_empty()
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
    fn test_attr_builder_file() {
        let config = test_permission_config();
        let builder = AttrBuilder::new(&config);

        let meta = ObjectMeta {
            path: "test/file.txt".to_string(),
            size: 1024,
            last_modified: None,
            is_dir: false,
            content_type: None,
            etag: None,
        };

        let attr = builder.from_object_meta(42, &meta);
        assert_eq!(attr.ino, 42);
        assert_eq!(attr.size, 1024);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.perm, 0o644);
    }

    #[test]
    fn test_attr_builder_directory() {
        let config = test_permission_config();
        let builder = AttrBuilder::new(&config);

        let meta = ObjectMeta {
            path: "test/dir/".to_string(),
            size: 0,
            last_modified: None,
            is_dir: true,
            content_type: None,
            etag: None,
        };

        let attr = builder.from_object_meta(42, &meta);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_setattr_flags() {
        let mut attr = FileAttr::default();
        attr.size = 100;
        attr.perm = 0o644;

        let flags = SetAttrFlags {
            size: Some(200),
            mode: Some(0o755),
            ..Default::default()
        };

        flags.apply(&mut attr);
        assert_eq!(attr.size, 200);
        assert_eq!(attr.perm, 0o755);
    }
}
