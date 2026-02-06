//! Comprehensive unit tests for Permission and Attribute modules
//!
//! Tests cover:
//! - Permission checking (read/write/execute)
//! - Permission metadata
//! - Attribute building
//! - SetAttr flags

#[cfg(test)]
mod permission_tests {
    use std::collections::HashMap;

    use obsfuse::config::{FixedPermission, PermissionConfig, PermissionMode};
    use obsfuse::fs::permission::PermissionManager;

    fn create_fixed_manager() -> PermissionManager {
        let config = PermissionConfig {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission {
                uid: 1000,
                gid: 1000,
                file_mode: 0o644,
                dir_mode: 0o755,
            },
        };
        PermissionManager::new(config)
    }

    fn create_preserved_manager() -> PermissionManager {
        let config = PermissionConfig {
            mode: PermissionMode::Preserved,
            fixed: FixedPermission::default(),
        };
        PermissionManager::new(config)
    }

    // ==================== Default Values ====================

    #[test]
    fn test_default_uid_gid() {
        let manager = create_fixed_manager();

        assert_eq!(manager.default_uid(), 1000);
        assert_eq!(manager.default_gid(), 1000);
    }

    #[test]
    fn test_default_modes() {
        let manager = create_fixed_manager();

        assert_eq!(manager.default_file_mode(), 0o644);
        assert_eq!(manager.default_dir_mode(), 0o755);
    }

    #[test]
    fn test_is_preserved_mode() {
        let fixed = create_fixed_manager();
        let preserved = create_preserved_manager();

        assert!(!fixed.is_preserved_mode());
        assert!(preserved.is_preserved_mode());
    }

    // ==================== Read Permission ====================

    #[test]
    fn test_check_read_owner() {
        let manager = create_fixed_manager();

        // Owner has read permission (0o644: rw-r--r--)
        assert!(manager.check_read(1000, 1000, 1000, 1000, 0o644));

        // Owner without read
        assert!(!manager.check_read(1000, 1000, 1000, 1000, 0o000));
    }

    #[test]
    fn test_check_read_group() {
        let manager = create_fixed_manager();

        // Different user, same group
        assert!(manager.check_read(2000, 1000, 1000, 1000, 0o644));

        // Group without read
        assert!(!manager.check_read(2000, 1000, 1000, 1000, 0o600));
    }

    #[test]
    fn test_check_read_other() {
        let manager = create_fixed_manager();

        // Different user and group
        assert!(manager.check_read(2000, 2000, 1000, 1000, 0o644));

        // Other without read
        assert!(!manager.check_read(2000, 2000, 1000, 1000, 0o640));
    }

    #[test]
    fn test_check_read_root() {
        let manager = create_fixed_manager();

        // Root can always read
        assert!(manager.check_read(0, 0, 1000, 1000, 0o000));
    }

    // ==================== Write Permission ====================

    #[test]
    fn test_check_write_owner() {
        let manager = create_fixed_manager();

        // Owner has write permission
        assert!(manager.check_write(1000, 1000, 1000, 1000, 0o644));

        // Owner without write
        assert!(!manager.check_write(1000, 1000, 1000, 1000, 0o444));
    }

    #[test]
    fn test_check_write_group() {
        let manager = create_fixed_manager();

        // Group without write (0o644)
        assert!(!manager.check_write(2000, 1000, 1000, 1000, 0o644));

        // Group with write (0o664)
        assert!(manager.check_write(2000, 1000, 1000, 1000, 0o664));
    }

    #[test]
    fn test_check_write_other() {
        let manager = create_fixed_manager();

        // Other without write (0o644)
        assert!(!manager.check_write(2000, 2000, 1000, 1000, 0o644));

        // Other with write (0o646)
        assert!(manager.check_write(2000, 2000, 1000, 1000, 0o646));
    }

    #[test]
    fn test_check_write_root() {
        let manager = create_fixed_manager();

        // Root can always write
        assert!(manager.check_write(0, 0, 1000, 1000, 0o000));
    }

    // ==================== Execute Permission ====================

    #[test]
    fn test_check_execute_owner() {
        let manager = create_fixed_manager();

        // Owner has execute
        assert!(manager.check_execute(1000, 1000, 1000, 1000, 0o755));

        // Owner without execute
        assert!(!manager.check_execute(1000, 1000, 1000, 1000, 0o644));
    }

    #[test]
    fn test_check_execute_group() {
        let manager = create_fixed_manager();

        // Group has execute
        assert!(manager.check_execute(2000, 1000, 1000, 1000, 0o755));

        // Group without execute
        assert!(!manager.check_execute(2000, 1000, 1000, 1000, 0o744));
    }

    #[test]
    fn test_check_execute_other() {
        let manager = create_fixed_manager();

        // Other has execute
        assert!(manager.check_execute(2000, 2000, 1000, 1000, 0o755));

        // Other without execute
        assert!(!manager.check_execute(2000, 2000, 1000, 1000, 0o754));
    }

    #[test]
    fn test_check_execute_root() {
        let manager = create_fixed_manager();

        // Root can execute if anyone can
        assert!(manager.check_execute(0, 0, 1000, 1000, 0o100));
        assert!(manager.check_execute(0, 0, 1000, 1000, 0o010));
        assert!(manager.check_execute(0, 0, 1000, 1000, 0o001));

        // Root cannot execute if no one can
        assert!(!manager.check_execute(0, 0, 1000, 1000, 0o000));
    }

    // ==================== Metadata ====================

    #[test]
    fn test_build_metadata_fixed_mode() {
        let manager = create_fixed_manager();

        let meta = manager.build_metadata(1000, 1000, 0o755);

        // Fixed mode doesn't store metadata
        assert!(meta.is_empty());
    }

    #[test]
    fn test_build_metadata_preserved_mode() {
        let manager = create_preserved_manager();

        let meta = manager.build_metadata(1000, 1000, 0o755);

        assert_eq!(meta.get("x-obs-meta-uid"), Some(&"1000".to_string()));
        assert_eq!(meta.get("x-obs-meta-gid"), Some(&"1000".to_string()));
        assert_eq!(meta.get("x-obs-meta-mode"), Some(&"493".to_string())); // 0o755 = 493
    }

    #[test]
    fn test_parse_metadata_fixed_mode() {
        let manager = create_fixed_manager();

        let mut meta = HashMap::new();
        meta.insert("x-obs-meta-uid".to_string(), "1000".to_string());
        meta.insert("x-obs-meta-gid".to_string(), "1000".to_string());
        meta.insert("x-obs-meta-mode".to_string(), "493".to_string());

        // Fixed mode ignores metadata
        assert!(manager.parse_metadata(&meta).is_none());
    }

    #[test]
    fn test_parse_metadata_preserved_mode() {
        let manager = create_preserved_manager();

        let mut meta = HashMap::new();
        meta.insert("x-obs-meta-uid".to_string(), "1000".to_string());
        meta.insert("x-obs-meta-gid".to_string(), "1000".to_string());
        meta.insert("x-obs-meta-mode".to_string(), "493".to_string());

        let result = manager.parse_metadata(&meta);
        assert!(result.is_some());

        let (uid, gid, mode) = result.unwrap();
        assert_eq!(uid, 1000);
        assert_eq!(gid, 1000);
        assert_eq!(mode, 493);
    }

    #[test]
    fn test_parse_metadata_missing_fields() {
        let manager = create_preserved_manager();

        let mut meta = HashMap::new();
        meta.insert("x-obs-meta-uid".to_string(), "1000".to_string());
        // Missing gid and mode

        assert!(manager.parse_metadata(&meta).is_none());
    }

    #[test]
    fn test_parse_metadata_invalid_values() {
        let manager = create_preserved_manager();

        let mut meta = HashMap::new();
        meta.insert("x-obs-meta-uid".to_string(), "not_a_number".to_string());
        meta.insert("x-obs-meta-gid".to_string(), "1000".to_string());
        meta.insert("x-obs-meta-mode".to_string(), "493".to_string());

        assert!(manager.parse_metadata(&meta).is_none());
    }
}

#[cfg(test)]
mod attr_tests {
    use std::time::SystemTime;

    use fuse3::FileType;

    use obsfuse::config::{FixedPermission, PermissionConfig, PermissionMode};
    use obsfuse::fs::attr::{AttrBuilder, SetAttrFlags, file_type_from_path, is_directory_path};
    use obsfuse::fs::inode::FileAttr;
    use obsfuse::storage::ObjectMeta;

    fn create_attr_builder() -> AttrBuilder<'static> {
        let config = Box::leak(Box::new(PermissionConfig {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission {
                uid: 1000,
                gid: 1000,
                file_mode: 0o644,
                dir_mode: 0o755,
            },
        }));
        AttrBuilder::new(config)
    }

    // ==================== AttrBuilder ====================

    #[test]
    fn test_from_object_meta_file() {
        let builder = create_attr_builder();

        let meta = ObjectMeta {
            path: "path/to/file.txt".to_string(),
            size: 1024,
            last_modified: Some(SystemTime::now()),
            is_dir: false,
            content_type: Some("text/plain".to_string()),
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
    fn test_from_object_meta_directory() {
        let builder = create_attr_builder();

        let meta = ObjectMeta {
            path: "path/to/dir/".to_string(),
            size: 0,
            last_modified: Some(SystemTime::now()),
            is_dir: true,
            content_type: None,
            etag: None,
        };

        let attr = builder.from_object_meta(42, &meta);

        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_new_file() {
        let builder = create_attr_builder();

        let attr = builder.new_file(42, None);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.size, 0);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.perm, 0o644);
    }

    #[test]
    fn test_new_file_custom_mode() {
        let builder = create_attr_builder();

        let attr = builder.new_file(42, Some(0o755));

        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_new_directory() {
        let builder = create_attr_builder();

        let attr = builder.new_directory(42, None);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_new_directory_custom_mode() {
        let builder = create_attr_builder();

        let attr = builder.new_directory(42, Some(0o700));

        assert_eq!(attr.perm, 0o700);
    }

    #[test]
    fn test_new_symlink() {
        let builder = create_attr_builder();

        let attr = builder.new_symlink(42, 20);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.size, 20);
        assert_eq!(attr.kind, FileType::Symlink);
        assert_eq!(attr.perm, 0o777);
    }

    // ==================== SetAttrFlags ====================

    #[test]
    fn test_setattr_flags_empty() {
        let flags = SetAttrFlags::default();

        assert!(!flags.has_any());
        assert!(!flags.is_truncate());
    }

    #[test]
    fn test_setattr_flags_has_any() {
        let flags = SetAttrFlags {
            mode: Some(0o755),
            ..Default::default()
        };

        assert!(flags.has_any());
    }

    #[test]
    fn test_setattr_flags_is_truncate() {
        let flags = SetAttrFlags {
            size: Some(0),
            ..Default::default()
        };

        assert!(flags.is_truncate());
    }

    #[test]
    fn test_setattr_flags_apply_mode() {
        let mut attr = FileAttr::default();
        attr.perm = 0o644;

        let flags = SetAttrFlags {
            mode: Some(0o755),
            ..Default::default()
        };

        flags.apply(&mut attr);

        assert_eq!(attr.perm, 0o755);
    }

    #[test]
    fn test_setattr_flags_apply_uid_gid() {
        let mut attr = FileAttr::default();
        attr.uid = 1000;
        attr.gid = 1000;

        let flags = SetAttrFlags {
            uid: Some(2000),
            gid: Some(2000),
            ..Default::default()
        };

        flags.apply(&mut attr);

        assert_eq!(attr.uid, 2000);
        assert_eq!(attr.gid, 2000);
    }

    #[test]
    fn test_setattr_flags_apply_size() {
        let mut attr = FileAttr::default();
        attr.size = 1024;

        let flags = SetAttrFlags {
            size: Some(0),
            ..Default::default()
        };

        flags.apply(&mut attr);

        assert_eq!(attr.size, 0);
        assert_eq!(attr.blocks, 0);
    }

    #[test]
    fn test_setattr_flags_apply_times() {
        let mut attr = FileAttr::default();
        let now = SystemTime::now();

        let flags = SetAttrFlags {
            atime: Some(now),
            mtime: Some(now),
            ..Default::default()
        };

        flags.apply(&mut attr);

        assert_eq!(attr.atime, now);
        assert_eq!(attr.mtime, now);
    }

    // ==================== Path Utilities ====================

    #[test]
    fn test_file_type_from_path() {
        assert_eq!(file_type_from_path("file.txt"), FileType::RegularFile);
        assert_eq!(file_type_from_path("dir/"), FileType::Directory);
        assert_eq!(file_type_from_path(""), FileType::Directory);
    }

    #[test]
    fn test_is_directory_path() {
        assert!(is_directory_path(""));
        assert!(is_directory_path("dir/"));
        assert!(!is_directory_path("file.txt"));
        assert!(!is_directory_path("path/to/file"));
    }
}
