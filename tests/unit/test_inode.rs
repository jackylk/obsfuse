//! Comprehensive unit tests for Inode Manager
//!
//! Tests cover:
//! - Inode creation and lookup
//! - Path to inode mapping
//! - Inode to path mapping
//! - Directory hierarchy
//! - Reference counting
//! - Rename operations
//! - Invalidation
//! - TTL expiration

use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;

    // Import test dependencies
    use obsfuse::config::{FixedPermission, PermissionConfig, PermissionMode};
    use obsfuse::fs::inode::{FileAttr, InodeEntry, InodeManager, ROOT_INODE};
    use fuse3::FileType;

    fn create_test_manager() -> InodeManager {
        let config = PermissionConfig {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission {
                uid: 1000,
                gid: 1000,
                file_mode: 0o644,
                dir_mode: 0o755,
            },
        };
        InodeManager::new(config)
    }

    // ==================== Basic Inode Operations ====================

    #[test]
    fn test_root_inode_exists() {
        let manager = create_test_manager();

        // Root inode should exist after creation
        let entry = manager.get_entry(ROOT_INODE);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert!(entry.is_dir);
        assert_eq!(entry.path, "");
        assert_eq!(entry.attr.kind, FileType::Directory);
    }

    #[test]
    fn test_root_inode_path_mapping() {
        let manager = create_test_manager();

        // Empty path should map to root inode
        assert_eq!(manager.get_inode(""), Some(ROOT_INODE));
        assert_eq!(manager.get_path(ROOT_INODE), Some(String::new()));
    }

    #[test]
    fn test_create_file_inode() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("test/file.txt", false, 1024);
        assert!(inode > ROOT_INODE);

        let entry = manager.get_entry(inode).unwrap();
        assert!(!entry.is_dir);
        assert_eq!(entry.path, "test/file.txt");
        assert_eq!(entry.attr.size, 1024);
        assert_eq!(entry.attr.kind, FileType::RegularFile);
    }

    #[test]
    fn test_create_directory_inode() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("test/dir", true, 0);
        assert!(inode > ROOT_INODE);

        let entry = manager.get_entry(inode).unwrap();
        assert!(entry.is_dir);
        assert_eq!(entry.path, "test/dir");
        assert_eq!(entry.attr.kind, FileType::Directory);
    }

    #[test]
    fn test_inode_idempotent() {
        let manager = create_test_manager();

        // Creating the same path twice should return the same inode
        let inode1 = manager.get_or_create_inode("test/file.txt", false, 100);
        let inode2 = manager.get_or_create_inode("test/file.txt", false, 100);

        assert_eq!(inode1, inode2);
    }

    #[test]
    fn test_different_paths_different_inodes() {
        let manager = create_test_manager();

        let inode1 = manager.get_or_create_inode("file1.txt", false, 100);
        let inode2 = manager.get_or_create_inode("file2.txt", false, 100);

        assert_ne!(inode1, inode2);
    }

    // ==================== Path Operations ====================

    #[test]
    fn test_get_inode_existing() {
        let manager = create_test_manager();

        manager.get_or_create_inode("path/to/file.txt", false, 100);

        assert!(manager.get_inode("path/to/file.txt").is_some());
    }

    #[test]
    fn test_get_inode_non_existing() {
        let manager = create_test_manager();

        assert!(manager.get_inode("non/existing/path").is_none());
    }

    #[test]
    fn test_get_path_existing() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("my/path.txt", false, 100);
        assert_eq!(manager.get_path(inode), Some("my/path.txt".to_string()));
    }

    #[test]
    fn test_get_path_non_existing() {
        let manager = create_test_manager();

        assert!(manager.get_path(99999).is_none());
    }

    #[test]
    fn test_path_normalization() {
        let manager = create_test_manager();

        // Leading and trailing slashes should be normalized
        let inode1 = manager.get_or_create_inode("path/to/file", false, 100);
        let inode2 = manager.get_or_create_inode("/path/to/file/", false, 100);

        // Both should resolve to the same normalized path
        assert_eq!(inode1, inode2);
    }

    // ==================== Path Utility Functions ====================

    #[test]
    fn test_parent_path() {
        assert_eq!(InodeManager::parent_path("a/b/c"), Some("a/b".to_string()));
        assert_eq!(InodeManager::parent_path("a/b"), Some("a".to_string()));
        assert_eq!(InodeManager::parent_path("a"), Some(String::new()));
        assert_eq!(InodeManager::parent_path(""), None);
    }

    #[test]
    fn test_file_name() {
        assert_eq!(InodeManager::file_name("a/b/c.txt"), "c.txt");
        assert_eq!(InodeManager::file_name("file.txt"), "file.txt");
        assert_eq!(InodeManager::file_name("dir/"), "dir");
        assert_eq!(InodeManager::file_name(""), "");
    }

    #[test]
    fn test_join_path() {
        assert_eq!(InodeManager::join_path("a/b", "c"), "a/b/c");
        assert_eq!(InodeManager::join_path("a", "b"), "a/b");
        assert_eq!(InodeManager::join_path("", "b"), "b");
        assert_eq!(InodeManager::join_path("a/", "b"), "a/b");
    }

    // ==================== Attribute Operations ====================

    #[test]
    fn test_update_size() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);
        manager.update_size(inode, 500);

        let entry = manager.get_entry(inode).unwrap();
        assert_eq!(entry.attr.size, 500);
    }

    #[test]
    fn test_update_attr() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);
        manager.update_attr(inode, |attr| {
            attr.size = 999;
            attr.perm = 0o755;
        });

        let entry = manager.get_entry(inode).unwrap();
        assert_eq!(entry.attr.size, 999);
        assert_eq!(entry.attr.perm, 0o755);
    }

    // ==================== Children Management ====================

    #[test]
    fn test_set_and_get_children() {
        let manager = create_test_manager();

        let dir_inode = manager.get_or_create_inode("dir", true, 0);
        let child1 = manager.get_or_create_inode("dir/file1.txt", false, 100);
        let child2 = manager.get_or_create_inode("dir/file2.txt", false, 200);

        manager.set_children(dir_inode, vec![child1, child2]);

        let children = manager.get_children(dir_inode);
        assert!(children.is_some());

        let children = children.unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1));
        assert!(children.contains(&child2));
    }

    #[test]
    fn test_get_children_none() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);
        assert!(manager.get_children(inode).is_none());
    }

    // ==================== Reference Counting ====================

    #[test]
    fn test_reference_counting() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);

        // Initial ref count should be 0
        let entry = manager.get_entry(inode).unwrap();
        assert_eq!(entry.ref_count, 0);

        // Increment
        manager.inc_ref(inode);
        manager.inc_ref(inode);
        let entry = manager.get_entry(inode).unwrap();
        assert_eq!(entry.ref_count, 2);

        // Decrement
        let count = manager.dec_ref(inode);
        assert_eq!(count, 1);

        let count = manager.dec_ref(inode);
        assert_eq!(count, 0);

        // Should not go negative
        let count = manager.dec_ref(inode);
        assert_eq!(count, 0);
    }

    // ==================== Rename Operations ====================

    #[test]
    fn test_rename() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("old/path.txt", false, 100);
        manager.rename("old/path.txt", "new/path.txt");

        // Old path should no longer exist
        assert!(manager.get_inode("old/path.txt").is_none());

        // New path should have the same inode
        assert_eq!(manager.get_inode("new/path.txt"), Some(inode));

        // Path should be updated in entry
        let entry = manager.get_entry(inode).unwrap();
        assert_eq!(entry.path, "new/path.txt");
    }

    // ==================== Remove Operations ====================

    #[test]
    fn test_remove_inode() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);
        assert!(manager.get_entry(inode).is_some());

        manager.remove(inode);

        assert!(manager.get_entry(inode).is_none());
        assert!(manager.get_inode("file.txt").is_none());
    }

    #[test]
    fn test_remove_by_path() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);
        manager.remove_by_path("file.txt");

        assert!(manager.get_entry(inode).is_none());
        assert!(manager.get_inode("file.txt").is_none());
    }

    // ==================== Invalidation ====================

    #[test]
    fn test_invalidate() {
        let manager = create_test_manager();

        let dir_inode = manager.get_or_create_inode("dir", true, 0);
        manager.set_children(dir_inode, vec![1, 2, 3]);

        manager.invalidate(dir_inode);

        // Children should be cleared
        assert!(manager.get_children(dir_inode).is_none());

        // Entry should be marked as expired
        assert!(manager.is_expired(dir_inode, Duration::from_secs(0)));
    }

    #[test]
    fn test_invalidate_path() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("path/to/file.txt", false, 100);
        manager.invalidate_path("path/to/file.txt");

        assert!(manager.is_expired(inode, Duration::from_secs(0)));
    }

    // ==================== TTL Expiration ====================

    #[test]
    fn test_is_expired_fresh() {
        let manager = create_test_manager();

        let inode = manager.get_or_create_inode("file.txt", false, 100);

        // Fresh entry should not be expired with reasonable TTL
        assert!(!manager.is_expired(inode, Duration::from_secs(60)));
    }

    #[test]
    fn test_is_expired_non_existing() {
        let manager = create_test_manager();

        // Non-existing inode should be considered expired
        assert!(manager.is_expired(99999, Duration::from_secs(60)));
    }

    // ==================== Statistics ====================

    #[test]
    fn test_stats() {
        let manager = create_test_manager();

        // Root inode exists initially
        let stats = manager.stats();
        assert_eq!(stats.total_inodes, 1);

        // Create more inodes
        manager.get_or_create_inode("file1.txt", false, 100);
        manager.get_or_create_inode("file2.txt", false, 100);
        manager.get_or_create_inode("dir/", true, 0);

        let stats = manager.stats();
        assert_eq!(stats.total_inodes, 4);
    }

    // ==================== FileAttr Tests ====================

    #[test]
    fn test_file_attr_directory() {
        let attr = FileAttr::directory(42, 1000, 1000, 0o755);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.perm, 0o755);
        assert_eq!(attr.nlink, 2);
    }

    #[test]
    fn test_file_attr_file() {
        let attr = FileAttr::file(42, 1024, 1000, 1000, 0o644);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.size, 1024);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.perm, 0o644);
        assert_eq!(attr.nlink, 1);
        assert_eq!(attr.blocks, 2); // (1024 + 511) / 512 = 2
    }

    #[test]
    fn test_file_attr_symlink() {
        let attr = FileAttr::symlink(42, 20, 1000, 1000);

        assert_eq!(attr.ino, 42);
        assert_eq!(attr.kind, FileType::Symlink);
        assert_eq!(attr.size, 20);
        assert_eq!(attr.perm, 0o777);
    }

    // ==================== Concurrent Access (basic) ====================

    #[test]
    fn test_concurrent_inode_creation() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(create_test_manager());
        let mut handles = vec![];

        for i in 0..10 {
            let manager = Arc::clone(&manager);
            handles.push(thread::spawn(move || {
                let path = format!("concurrent/file{}.txt", i);
                manager.get_or_create_inode(&path, false, 100)
            }));
        }

        let inodes: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All inodes should be unique
        let mut sorted = inodes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }
}
