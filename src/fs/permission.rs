//! Permission management for OBS FUSE filesystem
//!
//! This module handles permission checking and metadata storage
//! for both fixed and preserved permission modes.

use std::collections::HashMap;

use crate::config::{PermissionConfig, PermissionMode};

/// Permission manager
pub struct PermissionManager {
    config: PermissionConfig,
}

impl PermissionManager {
    /// Create a new permission manager
    pub fn new(config: PermissionConfig) -> Self {
        Self { config }
    }

    /// Get default UID
    pub fn default_uid(&self) -> u32 {
        self.config.fixed.uid
    }

    /// Get default GID
    pub fn default_gid(&self) -> u32 {
        self.config.fixed.gid
    }

    /// Get default file mode
    pub fn default_file_mode(&self) -> u32 {
        self.config.fixed.file_mode
    }

    /// Get default directory mode
    pub fn default_dir_mode(&self) -> u32 {
        self.config.fixed.dir_mode
    }

    /// Check if using preserved mode
    pub fn is_preserved_mode(&self) -> bool {
        matches!(self.config.mode, PermissionMode::Preserved)
    }

    /// Check read permission
    pub fn check_read(&self, uid: u32, gid: u32, file_uid: u32, file_gid: u32, mode: u16) -> bool {
        // Root can read anything
        if uid == 0 {
            return true;
        }

        // Owner check
        if uid == file_uid && (mode & 0o400) != 0 {
            return true;
        }

        // Group check
        if gid == file_gid && (mode & 0o040) != 0 {
            return true;
        }

        // Other check
        (mode & 0o004) != 0
    }

    /// Check write permission
    pub fn check_write(&self, uid: u32, gid: u32, file_uid: u32, file_gid: u32, mode: u16) -> bool {
        // Root can write anything
        if uid == 0 {
            return true;
        }

        // Owner check
        if uid == file_uid && (mode & 0o200) != 0 {
            return true;
        }

        // Group check
        if gid == file_gid && (mode & 0o020) != 0 {
            return true;
        }

        // Other check
        (mode & 0o002) != 0
    }

    /// Check execute permission
    pub fn check_execute(
        &self,
        uid: u32,
        gid: u32,
        file_uid: u32,
        file_gid: u32,
        mode: u16,
    ) -> bool {
        // Root can execute if anyone can
        if uid == 0 {
            return (mode & 0o111) != 0;
        }

        // Owner check
        if uid == file_uid && (mode & 0o100) != 0 {
            return true;
        }

        // Group check
        if gid == file_gid && (mode & 0o010) != 0 {
            return true;
        }

        // Other check
        (mode & 0o001) != 0
    }

    /// Build metadata for preserved permissions
    pub fn build_metadata(&self, uid: u32, gid: u32, mode: u32) -> HashMap<String, String> {
        let mut meta = HashMap::new();

        if self.is_preserved_mode() {
            meta.insert("x-obs-meta-uid".to_string(), uid.to_string());
            meta.insert("x-obs-meta-gid".to_string(), gid.to_string());
            meta.insert("x-obs-meta-mode".to_string(), mode.to_string());
        }

        meta
    }

    /// Parse permissions from OBS metadata
    pub fn parse_metadata(&self, meta: &HashMap<String, String>) -> Option<(u32, u32, u32)> {
        if !self.is_preserved_mode() {
            return None;
        }

        let uid = meta.get("x-obs-meta-uid")?.parse().ok()?;
        let gid = meta.get("x-obs-meta-gid")?.parse().ok()?;
        let mode = meta.get("x-obs-meta-mode")?.parse().ok()?;

        Some((uid, gid, mode))
    }
}

/// OBS metadata keys for permission storage
pub mod meta_keys {
    pub const UID: &str = "x-obs-meta-uid";
    pub const GID: &str = "x-obs-meta-gid";
    pub const MODE: &str = "x-obs-meta-mode";
    pub const SYMLINK_TARGET: &str = "x-obs-meta-symlink-target";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FixedPermission;

    fn test_config() -> PermissionConfig {
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
    fn test_permission_check_read() {
        let manager = PermissionManager::new(test_config());

        // Owner can read
        assert!(manager.check_read(1000, 1000, 1000, 1000, 0o644));

        // Group can read
        assert!(manager.check_read(2000, 1000, 1000, 1000, 0o644));

        // Other can read
        assert!(manager.check_read(2000, 2000, 1000, 1000, 0o644));

        // No read permission
        assert!(!manager.check_read(2000, 2000, 1000, 1000, 0o000));

        // Root can always read
        assert!(manager.check_read(0, 0, 1000, 1000, 0o000));
    }

    #[test]
    fn test_permission_check_write() {
        let manager = PermissionManager::new(test_config());

        // Owner can write
        assert!(manager.check_write(1000, 1000, 1000, 1000, 0o644));

        // Group cannot write with 0o644
        assert!(!manager.check_write(2000, 1000, 1000, 1000, 0o644));

        // Group can write with 0o664
        assert!(manager.check_write(2000, 1000, 1000, 1000, 0o664));

        // Root can always write
        assert!(manager.check_write(0, 0, 1000, 1000, 0o000));
    }

    #[test]
    fn test_preserved_mode_metadata() {
        let mut config = test_config();
        config.mode = PermissionMode::Preserved;
        let manager = PermissionManager::new(config);

        let meta = manager.build_metadata(1000, 1000, 0o755);
        assert_eq!(meta.get("x-obs-meta-uid"), Some(&"1000".to_string()));
        assert_eq!(meta.get("x-obs-meta-gid"), Some(&"1000".to_string()));
        assert_eq!(meta.get("x-obs-meta-mode"), Some(&"493".to_string())); // 0o755 = 493

        let parsed = manager.parse_metadata(&meta);
        assert_eq!(parsed, Some((1000, 1000, 493)));
    }
}
