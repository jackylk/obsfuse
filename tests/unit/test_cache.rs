//! Comprehensive unit tests for Cache modules
//!
//! Tests cover:
//! - Metadata cache (attributes, directories, negative cache)
//! - Data cache (block storage, LRU eviction)
//! - Read-ahead (sequential detection, prefetch)
//! - Write buffer (buffering, flush, multipart)

#[cfg(test)]
mod metadata_cache_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::Bytes;
    use bytesize::ByteSize;
    use fuse3::FileType;

    use obsfuse::cache::{DataCache, MetadataCache, ReadaheadManager, WriteBufferConfig};
    use obsfuse::config::{CacheConfig, MetadataCacheConfig};
    use obsfuse::fs::inode::FileAttr;
    use obsfuse::utils::Metrics;

    fn create_metadata_cache() -> MetadataCache {
        let config = MetadataCacheConfig {
            attr_ttl: Duration::from_secs(60),
            dir_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_secs(30),
            max_entries: 10000,
        };
        MetadataCache::new(config, Arc::new(Metrics::new()))
    }

    // ==================== Attribute Cache Tests ====================

    #[test]
    fn test_attr_cache_put_and_get() {
        let cache = create_metadata_cache();

        let attr = FileAttr {
            ino: 42,
            size: 1024,
            kind: FileType::RegularFile,
            perm: 0o644,
            ..Default::default()
        };

        cache.put_attr(42, attr.clone());

        let cached = cache.get_attr(42);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().size, 1024);
    }

    #[test]
    fn test_attr_cache_miss() {
        let cache = create_metadata_cache();
        assert!(cache.get_attr(99999).is_none());
    }

    #[test]
    fn test_attr_cache_invalidate() {
        let cache = create_metadata_cache();

        let attr = FileAttr::default();
        cache.put_attr(42, attr);
        assert!(cache.get_attr(42).is_some());

        cache.invalidate_attr(42);
        assert!(cache.get_attr(42).is_none());
    }

    #[test]
    fn test_attr_cache_update() {
        let cache = create_metadata_cache();

        let attr = FileAttr {
            ino: 42,
            size: 100,
            ..Default::default()
        };
        cache.put_attr(42, attr);

        cache.update_attr(42, |a| {
            a.size = 500;
        });

        let cached = cache.get_attr(42).unwrap();
        assert_eq!(cached.size, 500);
    }

    #[test]
    fn test_attr_cache_expiry() {
        let config = MetadataCacheConfig {
            attr_ttl: Duration::from_millis(1), // Very short TTL
            dir_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_secs(30),
            max_entries: 10000,
        };
        let cache = MetadataCache::new(config, Arc::new(Metrics::new()));

        let attr = FileAttr::default();
        cache.put_attr(42, attr);

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(10));

        // Should be expired
        assert!(cache.get_attr(42).is_none());
    }

    // ==================== Directory Cache Tests ====================

    #[test]
    fn test_dir_cache_put_and_get() {
        let cache = create_metadata_cache();

        cache.put_dir(1, vec![2, 3, 4, 5], true);

        let cached = cache.get_dir(1);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn test_dir_cache_miss() {
        let cache = create_metadata_cache();
        assert!(cache.get_dir(99999).is_none());
    }

    #[test]
    fn test_dir_cache_invalidate() {
        let cache = create_metadata_cache();

        cache.put_dir(1, vec![2, 3], true);
        cache.invalidate_dir(1);

        assert!(cache.get_dir(1).is_none());
    }

    // ==================== Negative Cache Tests ====================

    #[test]
    fn test_negative_cache() {
        let cache = create_metadata_cache();

        assert!(!cache.is_negative("/nonexistent"));

        cache.put_negative("/nonexistent");
        assert!(cache.is_negative("/nonexistent"));

        cache.remove_negative("/nonexistent");
        assert!(!cache.is_negative("/nonexistent"));
    }

    #[test]
    fn test_negative_cache_expiry() {
        let config = MetadataCacheConfig {
            attr_ttl: Duration::from_secs(60),
            dir_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_millis(1), // Very short
            max_entries: 10000,
        };
        let cache = MetadataCache::new(config, Arc::new(Metrics::new()));

        cache.put_negative("/path");

        std::thread::sleep(Duration::from_millis(10));

        assert!(!cache.is_negative("/path"));
    }

    // ==================== Clear and Stats Tests ====================

    #[test]
    fn test_cache_clear() {
        let cache = create_metadata_cache();

        cache.put_attr(1, FileAttr::default());
        cache.put_attr(2, FileAttr::default());
        cache.put_dir(1, vec![2], true);
        cache.put_negative("/path");

        cache.clear();

        assert!(cache.get_attr(1).is_none());
        assert!(cache.get_attr(2).is_none());
        assert!(cache.get_dir(1).is_none());
        assert!(!cache.is_negative("/path"));
    }

    #[test]
    fn test_cache_stats() {
        let cache = create_metadata_cache();

        cache.put_attr(1, FileAttr::default());
        cache.put_attr(2, FileAttr::default());
        cache.put_dir(1, vec![2], true);
        cache.put_negative("/path");

        let stats = cache.stats();
        assert_eq!(stats.attr_entries, 2);
        assert_eq!(stats.dir_entries, 1);
        assert_eq!(stats.negative_entries, 1);
    }
}

#[cfg(test)]
mod data_cache_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::Bytes;
    use bytesize::ByteSize;

    use obsfuse::cache::{BlockKey, DataCache};
    use obsfuse::config::CacheConfig;
    use obsfuse::utils::Metrics;

    fn create_data_cache(memory_limit_mb: u64, block_size_kb: u64) -> DataCache {
        let config = CacheConfig {
            memory_limit: ByteSize::mb(memory_limit_mb),
            disk_limit: ByteSize::mb(0), // No disk cache for tests
            cache_dir: None,
            block_size: ByteSize::kb(block_size_kb),
            metadata: Default::default(),
        };
        DataCache::new(&config, Arc::new(Metrics::new()))
    }

    // ==================== Block Key Tests ====================

    #[test]
    fn test_block_key_alignment() {
        // Block size = 1024
        let key = BlockKey::new(1, 500, 1024);
        assert_eq!(key.offset, 0); // Aligned to block boundary

        let key = BlockKey::new(1, 1024, 1024);
        assert_eq!(key.offset, 1024);

        let key = BlockKey::new(1, 1500, 1024);
        assert_eq!(key.offset, 1024);

        let key = BlockKey::new(1, 2048, 1024);
        assert_eq!(key.offset, 2048);
    }

    #[test]
    fn test_block_key_block_num() {
        let key = BlockKey { inode: 1, offset: 0 };
        assert_eq!(key.block_num(1024), 0);

        let key = BlockKey { inode: 1, offset: 1024 };
        assert_eq!(key.block_num(1024), 1);

        let key = BlockKey { inode: 1, offset: 4096 };
        assert_eq!(key.block_num(1024), 4);
    }

    // ==================== Cache Operations ====================

    #[test]
    fn test_data_cache_put_and_get() {
        let cache = create_data_cache(10, 64);

        let data = Bytes::from(vec![0u8; 1024]);
        cache.put(1, 0, data.clone());

        let cached = cache.get(1, 0);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1024);
    }

    #[test]
    fn test_data_cache_miss() {
        let cache = create_data_cache(10, 64);
        assert!(cache.get(1, 0).is_none());
    }

    #[test]
    fn test_data_cache_different_offsets() {
        let cache = create_data_cache(10, 64);

        let data1 = Bytes::from(vec![1u8; 1024]);
        let data2 = Bytes::from(vec![2u8; 1024]);

        cache.put(1, 0, data1);
        cache.put(1, 65536, data2); // Different block

        let cached1 = cache.get(1, 0).unwrap();
        let cached2 = cache.get(1, 65536).unwrap();

        assert_eq!(cached1[0], 1);
        assert_eq!(cached2[0], 2);
    }

    #[test]
    fn test_data_cache_invalidate_inode() {
        let cache = create_data_cache(10, 64);

        cache.put(1, 0, Bytes::from(vec![0u8; 1024]));
        cache.put(1, 65536, Bytes::from(vec![0u8; 1024]));
        cache.put(2, 0, Bytes::from(vec![0u8; 1024]));

        cache.invalidate(1);

        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(1, 65536).is_none());
        assert!(cache.get(2, 0).is_some()); // Different inode
    }

    #[test]
    fn test_data_cache_invalidate_range() {
        let cache = create_data_cache(10, 1); // 1KB blocks

        // Create blocks at 0, 1024, 2048, 3072
        for i in 0..4 {
            cache.put(1, i * 1024, Bytes::from(vec![i as u8; 1024]));
        }

        // Invalidate range from 1000 to 2500 (affects blocks 0, 1024, 2048)
        cache.invalidate_range(1, 1000, 1500);

        // Blocks 0 and 2048 should be invalidated
        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(1, 1024).is_none());
        assert!(cache.get(1, 2048).is_none());
        assert!(cache.get(1, 3072).is_some()); // Outside range
    }

    #[test]
    fn test_data_cache_eviction() {
        // Small cache that can hold ~5 blocks of 1KB each
        let cache = create_data_cache(1, 1); // 1MB limit, 1KB blocks

        // But we'll use smaller data to test eviction
        let config = CacheConfig {
            memory_limit: ByteSize::kb(5),
            disk_limit: ByteSize::mb(0),
            cache_dir: None,
            block_size: ByteSize::kb(1),
            metadata: Default::default(),
        };
        let cache = DataCache::new(&config, Arc::new(Metrics::new()));

        // Add more blocks than can fit
        for i in 0..20 {
            cache.put(1, i * 1024, Bytes::from(vec![i as u8; 1024]));
        }

        // Some blocks should have been evicted
        let stats = cache.stats();
        assert!(stats.memory_usage <= 5 * 1024);
    }

    #[test]
    fn test_data_cache_clear() {
        let cache = create_data_cache(10, 64);

        cache.put(1, 0, Bytes::from(vec![0u8; 1024]));
        cache.put(2, 0, Bytes::from(vec![0u8; 1024]));

        cache.clear();

        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(2, 0).is_none());

        let stats = cache.stats();
        assert_eq!(stats.memory_entries, 0);
        assert_eq!(stats.memory_usage, 0);
    }

    #[test]
    fn test_data_cache_stats() {
        let cache = create_data_cache(10, 64);

        let data = Bytes::from(vec![0u8; 1024]);
        cache.put(1, 0, data.clone());
        cache.put(1, 65536, data.clone());

        let stats = cache.stats();
        assert_eq!(stats.memory_entries, 2);
        assert_eq!(stats.memory_usage, 2048);
    }
}

#[cfg(test)]
mod readahead_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::Bytes;

    use obsfuse::cache::{ReadaheadConfig, ReadaheadManager};
    use obsfuse::utils::Metrics;

    fn create_readahead_manager() -> ReadaheadManager {
        let config = ReadaheadConfig {
            enable: true,
            window_size: 16 * 1024 * 1024, // 16MB
            concurrency: 4,
            seq_threshold: 3,
            prefetch_ttl: Duration::from_secs(30),
        };
        ReadaheadManager::new(config, Arc::new(Metrics::new()))
    }

    #[test]
    fn test_readahead_disabled() {
        let config = ReadaheadConfig {
            enable: false,
            ..Default::default()
        };
        let manager = ReadaheadManager::new(config, Arc::new(Metrics::new()));

        assert!(!manager.is_enabled());

        // Should never trigger prefetch when disabled
        for i in 0..10 {
            let request = manager.record_read(1, i * 1024, 1024);
            assert!(request.is_none());
        }
    }

    #[test]
    fn test_sequential_read_detection() {
        let manager = create_readahead_manager();

        // First few reads don't trigger prefetch
        let req1 = manager.record_read(1, 0, 1024);
        let req2 = manager.record_read(1, 1024, 1024);
        assert!(req1.is_none());
        assert!(req2.is_none());

        // Third sequential read should trigger prefetch
        let req3 = manager.record_read(1, 2048, 1024);
        assert!(req3.is_some());

        let request = req3.unwrap();
        assert_eq!(request.inode, 1);
        assert_eq!(request.offset, 3072); // Next offset after read
    }

    #[test]
    fn test_non_sequential_read_no_prefetch() {
        let manager = create_readahead_manager();

        // Random reads should not trigger prefetch
        manager.record_read(1, 0, 1024);
        manager.record_read(1, 10000, 1024);
        let req = manager.record_read(1, 5000, 1024);

        assert!(req.is_none());
    }

    #[test]
    fn test_prefetch_storage() {
        let manager = create_readahead_manager();

        let data = Bytes::from(vec![42u8; 1024]);
        manager.store_prefetched(1, 1000, data.clone());

        let retrieved = manager.get_prefetched(1, 1000);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().len(), 1024);
    }

    #[test]
    fn test_prefetch_miss() {
        let manager = create_readahead_manager();
        assert!(manager.get_prefetched(1, 0).is_none());
    }

    #[test]
    fn test_prefetch_invalidate() {
        let manager = create_readahead_manager();

        manager.store_prefetched(1, 0, Bytes::from(vec![0u8; 1024]));
        manager.store_prefetched(1, 1024, Bytes::from(vec![0u8; 1024]));
        manager.store_prefetched(2, 0, Bytes::from(vec![0u8; 1024]));

        manager.invalidate(1);

        assert!(manager.get_prefetched(1, 0).is_none());
        assert!(manager.get_prefetched(1, 1024).is_none());
        assert!(manager.get_prefetched(2, 0).is_some()); // Different inode
    }

    #[test]
    fn test_prefetch_clear() {
        let manager = create_readahead_manager();

        manager.store_prefetched(1, 0, Bytes::from(vec![0u8; 1024]));
        manager.store_prefetched(2, 0, Bytes::from(vec![0u8; 1024]));

        manager.clear();

        assert!(manager.get_prefetched(1, 0).is_none());
        assert!(manager.get_prefetched(2, 0).is_none());
    }

    #[test]
    fn test_prefetch_stats() {
        let manager = create_readahead_manager();

        // Simulate sequential reads for file 1
        manager.record_read(1, 0, 1024);
        manager.record_read(1, 1024, 1024);

        // Store some prefetched data
        manager.store_prefetched(1, 5000, Bytes::from(vec![0u8; 1024]));
        manager.store_prefetched(2, 0, Bytes::from(vec![0u8; 1024]));

        let stats = manager.stats();
        assert_eq!(stats.tracked_files, 1); // Only file 1 has read state
        assert_eq!(stats.prefetched_blocks, 2);
    }

    #[test]
    fn test_multiple_files_sequential() {
        let manager = create_readahead_manager();

        // Sequential reads on file 1
        manager.record_read(1, 0, 1024);
        manager.record_read(1, 1024, 1024);
        let req1 = manager.record_read(1, 2048, 1024);

        // Sequential reads on file 2
        manager.record_read(2, 0, 1024);
        manager.record_read(2, 1024, 1024);
        let req2 = manager.record_read(2, 2048, 1024);

        // Both should trigger prefetch
        assert!(req1.is_some());
        assert!(req2.is_some());

        // Verify they're for different files
        assert_eq!(req1.unwrap().inode, 1);
        assert_eq!(req2.unwrap().inode, 2);
    }
}

#[cfg(test)]
mod write_buffer_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use obsfuse::cache::{FileWriteBuffer, WriteBufferConfig};

    #[test]
    fn test_file_write_buffer_creation() {
        let buf = FileWriteBuffer::new(1, "test.txt".to_string(), 0);

        assert_eq!(buf.inode, 1);
        assert_eq!(buf.path, "test.txt");
        assert!(!buf.dirty);
        assert_eq!(buf.size, 0);
        assert!(buf.multipart.is_none());
    }

    #[test]
    fn test_file_write_buffer_needs_flush() {
        let mut buf = FileWriteBuffer::new(1, "test.txt".to_string(), 0);

        assert!(!buf.needs_flush(1024));

        buf.data.extend_from_slice(&[0u8; 500]);
        assert!(!buf.needs_flush(1024));

        buf.data.extend_from_slice(&[0u8; 600]);
        assert!(buf.needs_flush(1024));
    }

    #[test]
    fn test_file_write_buffer_needs_time_flush() {
        let mut buf = FileWriteBuffer::new(1, "test.txt".to_string(), 0);

        // Not dirty, should not need flush
        assert!(!buf.needs_time_flush(Duration::from_millis(1)));

        // Mark dirty
        buf.dirty = true;

        // Short interval, might not trigger
        assert!(!buf.needs_time_flush(Duration::from_secs(60)));

        // After waiting
        std::thread::sleep(Duration::from_millis(10));
        assert!(buf.needs_time_flush(Duration::from_millis(1)));
    }

    #[test]
    fn test_write_buffer_config_default() {
        let config = WriteBufferConfig::default();

        assert_eq!(config.buffer_size, 64 * 1024 * 1024);
        assert_eq!(config.multipart_threshold, 100 * 1024 * 1024);
        assert_eq!(config.part_size, 8 * 1024 * 1024);
    }
}
