//! Strong consistency tests for filesystem operations
//!
//! These tests verify that all filesystem operations maintain strong consistency
//! guarantees even under concurrent access.

use bytes::Bytes;
use bytesize::ByteSize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use obsfuse::cache::{DataCache, MetadataCache};
use obsfuse::config::{CacheConfig, FixedPermission, MetadataCacheConfig, PermissionConfig, PermissionMode};
use obsfuse::fs::inode::FileAttr;
use obsfuse::fs::InodeManager;
use obsfuse::utils::Metrics;

// ============================================================================
// Consistency Test Helpers
// ============================================================================

fn create_inode_manager() -> Arc<InodeManager> {
    let config = PermissionConfig {
        mode: PermissionMode::Fixed,
        fixed: FixedPermission {
            uid: 1000,
            gid: 1000,
            file_mode: 0o644,
            dir_mode: 0o755,
        },
    };
    Arc::new(InodeManager::new(config))
}

fn create_metadata_cache() -> Arc<MetadataCache> {
    let config = MetadataCacheConfig {
        attr_ttl: Duration::from_secs(60),
        dir_ttl: Duration::from_secs(60),
        negative_ttl: Duration::from_secs(60),
        max_entries: 100000,
    };
    Arc::new(MetadataCache::new(config, Arc::new(Metrics::new())))
}

fn create_data_cache() -> Arc<DataCache> {
    let config = CacheConfig {
        memory_limit: ByteSize::mb(256),
        disk_limit: ByteSize::mb(0),
        cache_dir: None,
        block_size: ByteSize::kb(64),
        metadata: Default::default(),
    };
    Arc::new(DataCache::new(&config, Arc::new(Metrics::new())))
}

// ============================================================================
// Read-After-Write Consistency Tests
// ============================================================================

/// Test: After writing to inode manager, reading must return the written data
#[test]
fn test_consistency_inode_read_after_write() {
    let manager = create_inode_manager();

    for i in 0..1000 {
        let path = format!("consistency/test/{}", i);
        let size = (i * 1024) as u64;

        // Write
        let inode = manager.get_or_create_inode(&path, false, size);

        // Read - must match
        let read_inode = manager.get_inode(&path);
        assert_eq!(read_inode, Some(inode), "Inode mismatch for path {}", path);

        let entry = manager.get_entry(inode);
        assert!(entry.is_some(), "Entry not found for inode {}", inode);
        assert_eq!(entry.unwrap().attr.size, size, "Size mismatch for inode {}", inode);
    }
}

/// Test: Concurrent writes to different paths, each read returns correct data
#[test]
fn test_consistency_inode_concurrent_raw() {
    let manager = create_inode_manager();
    let num_threads = 8;
    let ops_per_thread = 1000;
    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait(); // Synchronize start

                let mut created = Vec::new();
                for i in 0..ops_per_thread {
                    let path = format!("thread{}/file{}", t, i);
                    let size = ((t * 1000 + i) * 100) as u64;
                    let inode = manager.get_or_create_inode(&path, false, size);
                    created.push((path, inode, size));
                }

                // Verify all created entries
                for (path, expected_inode, expected_size) in created {
                    let inode = manager.get_inode(&path);
                    assert_eq!(inode, Some(expected_inode),
                        "Thread {}: Inode mismatch for {}", t, path);

                    let entry = manager.get_entry(expected_inode);
                    assert!(entry.is_some(),
                        "Thread {}: Entry not found for {}", t, path);
                    assert_eq!(entry.unwrap().attr.size, expected_size,
                        "Thread {}: Size mismatch for {}", t, path);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Thread panicked");
    }
}

/// Test: Metadata cache read-after-write consistency
#[test]
fn test_consistency_metadata_cache_raw() {
    let cache = create_metadata_cache();

    for i in 0..1000 {
        let mut attr = FileAttr::default();
        attr.ino = i;
        attr.size = i * 1024;

        // Write
        cache.put_attr(i, attr.clone());

        // Read - must match
        let cached = cache.get_attr(i);
        assert!(cached.is_some(), "Cache miss for inode {}", i);
        assert_eq!(cached.unwrap().size, attr.size, "Size mismatch for inode {}", i);
    }
}

/// Test: Data cache read-after-write consistency
#[test]
fn test_consistency_data_cache_raw() {
    let cache = create_data_cache();

    for i in 0..100 {
        let data = Bytes::from(vec![(i % 256) as u8; 65536]);
        let offset = i as u64 * 65536;

        // Write
        cache.put(1, offset, data.clone());

        // Read - must match
        let cached = cache.get(1, offset);
        assert!(cached.is_some(), "Cache miss for offset {}", offset);
        assert_eq!(cached.unwrap().as_ref(), data.as_ref(),
            "Data mismatch for offset {}", offset);
    }
}

// ============================================================================
// Overwrite Consistency Tests
// ============================================================================

/// Test: Overwrite must be visible immediately
#[test]
fn test_consistency_inode_overwrite() {
    let manager = create_inode_manager();
    let path = "overwrite/test";

    // Initial write
    let inode = manager.get_or_create_inode(path, false, 100);

    // Overwrite (update size)
    manager.update_size(inode, 500);

    // Read - must see new value
    let entry = manager.get_entry(inode).unwrap();
    assert_eq!(entry.attr.size, 500, "Overwrite not visible");
}

/// Test: Metadata cache overwrite consistency
#[test]
fn test_consistency_metadata_cache_overwrite() {
    let cache = create_metadata_cache();

    // Initial write
    let mut attr = FileAttr::default();
    attr.ino = 1;
    attr.size = 100;
    cache.put_attr(1, attr);

    // Overwrite
    let mut new_attr = FileAttr::default();
    new_attr.ino = 1;
    new_attr.size = 500;
    cache.put_attr(1, new_attr);

    // Read - must see new value
    let cached = cache.get_attr(1).unwrap();
    assert_eq!(cached.size, 500, "Overwrite not visible in cache");
}

// ============================================================================
// Delete Visibility Tests
// ============================================================================

/// Test: Delete must be visible immediately
#[test]
fn test_consistency_inode_delete_invisible() {
    let manager = create_inode_manager();
    let path = "delete/test";

    // Create
    let inode = manager.get_or_create_inode(path, false, 100);
    assert!(manager.get_inode(path).is_some());

    // Delete
    manager.remove(inode);

    // Must be invisible
    assert!(manager.get_inode(path).is_none(), "Deleted inode still visible by path");
    assert!(manager.get_entry(inode).is_none(), "Deleted entry still visible");
}

/// Test: Cache invalidation after delete
#[test]
fn test_consistency_cache_invalidate_after_delete() {
    let cache = create_metadata_cache();

    // Create
    cache.put_attr(1, FileAttr::default());
    assert!(cache.get_attr(1).is_some());

    // Invalidate
    cache.invalidate_attr(1);

    // Must be invisible
    assert!(cache.get_attr(1).is_none(), "Invalidated cache entry still visible");
}

// ============================================================================
// Rename Atomicity Tests
// ============================================================================

/// Test: Rename is atomic - file is always at exactly one path
#[test]
fn test_consistency_rename_atomic() {
    let manager = create_inode_manager();
    let old_path = "rename/old";
    let new_path = "rename/new";

    // Create at old path
    let inode = manager.get_or_create_inode(old_path, false, 100);

    // Rename
    manager.rename(old_path, new_path);

    // Must be at new path only
    assert!(manager.get_inode(old_path).is_none(), "File still at old path");
    assert_eq!(manager.get_inode(new_path), Some(inode), "File not at new path");
}

/// Test: Concurrent renames don't lose data
#[test]
fn test_consistency_concurrent_rename() {
    let manager = create_inode_manager();
    let num_threads = 8;
    let ops_per_thread = 100;

    // Create initial files
    for t in 0..num_threads {
        for i in 0..ops_per_thread {
            let path = format!("rename_test/t{}/f{}", t, i);
            manager.get_or_create_inode(&path, false, 100);
        }
    }

    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();

                for i in 0..ops_per_thread {
                    let old_path = format!("rename_test/t{}/f{}", t, i);
                    let new_path = format!("rename_test/t{}/renamed_{}", t, i);
                    manager.rename(&old_path, &new_path);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Thread panicked");
    }

    // Verify all files are renamed
    for t in 0..num_threads {
        for i in 0..ops_per_thread {
            let old_path = format!("rename_test/t{}/f{}", t, i);
            let new_path = format!("rename_test/t{}/renamed_{}", t, i);

            assert!(manager.get_inode(&old_path).is_none(),
                "File still at old path: {}", old_path);
            assert!(manager.get_inode(&new_path).is_some(),
                "File not at new path: {}", new_path);
        }
    }
}

// ============================================================================
// Negative Cache Consistency Tests
// ============================================================================

/// Test: Creating a file invalidates negative cache
#[test]
fn test_consistency_negative_cache_invalidate() {
    let cache = create_metadata_cache();
    let path = "/nonexistent/file";

    // Mark as non-existent
    cache.put_negative(path);
    assert!(cache.is_negative(path));

    // "Create" the file (remove from negative cache)
    cache.remove_negative(path);

    // Must no longer be in negative cache
    assert!(!cache.is_negative(path), "Negative cache not cleared after create");
}

// ============================================================================
// Concurrent Consistency Verification
// ============================================================================

/// Test: High contention concurrent access maintains consistency
#[test]
fn test_consistency_high_contention() {
    let manager = create_inode_manager();
    let cache = create_metadata_cache();
    let num_threads = 16;
    let ops_per_thread = 500;
    let shared_paths = 100; // Hot paths accessed by all threads

    // Create shared paths
    for i in 0..shared_paths {
        let path = format!("shared/{}", i);
        let inode = manager.get_or_create_inode(&path, false, i as u64 * 100);
        cache.put_attr(inode, FileAttr::default());
    }

    let success_count = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let manager = Arc::clone(&manager);
            let _cache = Arc::clone(&cache);
            let success_count = Arc::clone(&success_count);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                barrier.wait();

                for i in 0..ops_per_thread {
                    let idx = (t * 7 + i * 13) % shared_paths; // Pseudo-random access
                    let path = format!("shared/{}", idx);

                    // Read and verify
                    if let Some(inode) = manager.get_inode(&path) {
                        if let Some(entry) = manager.get_entry(inode) {
                            if entry.path == path {
                                success_count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Thread panicked");
    }

    let total_ops = num_threads * ops_per_thread;
    let successes = success_count.load(Ordering::Relaxed);
    assert_eq!(successes, total_ops as u64,
        "Consistency violations: {} of {} operations failed",
        total_ops as u64 - successes, total_ops);
}

/// Test: No duplicate inodes for same path under concurrent creation
#[test]
fn test_consistency_no_duplicate_inodes() {
    let manager = create_inode_manager();
    let num_threads = 16;
    let path = "same/path/for/all";
    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let manager = Arc::clone(&manager);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                manager.get_or_create_inode(path, false, 100)
            })
        })
        .collect();

    let inodes: Vec<u64> = handles.into_iter()
        .map(|h| h.join().expect("Thread panicked"))
        .collect();

    // All threads must get the same inode
    let unique: HashSet<_> = inodes.iter().collect();
    assert_eq!(unique.len(), 1,
        "Multiple inodes created for same path: {:?}", inodes);
}
