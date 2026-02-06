//! Integration tests for OBS FUSE filesystem
//!
//! These tests verify the complete system behavior.
//! Some tests require a real OBS bucket and credentials.
//!
//! Set the following environment variables for OBS tests:
//! - OBS_BUCKET
//! - OBS_ACCESS_KEY
//! - OBS_SECRET_KEY
//! - OBS_ENDPOINT (optional)

use std::sync::Arc;

/// Skip test if OBS credentials are not available
fn skip_if_no_credentials() -> bool {
    std::env::var("OBS_BUCKET").is_err()
        || std::env::var("OBS_ACCESS_KEY").is_err()
        || std::env::var("OBS_SECRET_KEY").is_err()
}

/// Helper to create test configuration
fn create_test_config() -> obsfuse::Config {
    let mut config = obsfuse::Config::default();
    config.obs.bucket = std::env::var("OBS_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());
    config.obs.access_key = std::env::var("OBS_ACCESS_KEY").ok();
    config.obs.secret_key = std::env::var("OBS_SECRET_KEY").ok();
    if let Ok(endpoint) = std::env::var("OBS_ENDPOINT") {
        config.obs.endpoint = endpoint;
    }
    config
}

#[cfg(test)]
mod config_tests {
    use obsfuse::Config;

    #[test]
    fn test_config_default_creation() {
        let config = Config::default();
        assert!(!config.obs.endpoint.is_empty());
        assert!(config.cache.memory_limit.as_u64() > 0);
    }

    #[test]
    fn test_config_env_merge() {
        std::env::set_var("OBS_ACCESS_KEY_TEST", "test_ak");

        let mut config = Config::default();
        // This doesn't actually merge OBS_ACCESS_KEY_TEST, but tests the merge logic
        config.merge_env();

        // Cleanup
        std::env::remove_var("OBS_ACCESS_KEY_TEST");
    }
}

#[cfg(test)]
mod metrics_tests {
    use obsfuse::Metrics;

    #[test]
    fn test_metrics_collection() {
        let metrics = Metrics::new();

        metrics.inc_read_ops();
        metrics.inc_read_ops();
        metrics.add_read_bytes(1024);
        metrics.inc_write_ops();
        metrics.add_write_bytes(512);

        let summary = metrics.summary();
        assert_eq!(summary.read_ops, 2);
        assert_eq!(summary.read_bytes, 1024);
        assert_eq!(summary.write_ops, 1);
        assert_eq!(summary.write_bytes, 512);
    }

    #[test]
    fn test_cache_hit_rates() {
        let metrics = Metrics::new();

        // Simulate 75% hit rate
        metrics.inc_read_cache_hit();
        metrics.inc_read_cache_hit();
        metrics.inc_read_cache_hit();
        metrics.inc_read_cache_miss();

        let hit_rate = metrics.read_cache_hit_rate();
        assert!((hit_rate - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_metrics_summary_display() {
        let metrics = Metrics::new();
        metrics.inc_obs_get();
        metrics.inc_obs_put();

        let summary = metrics.summary();
        let display = format!("{}", summary);

        assert!(display.contains("OBS Operations"));
        assert!(display.contains("GET: 1"));
        assert!(display.contains("PUT: 1"));
    }
}

#[cfg(test)]
mod inode_integration_tests {
    use obsfuse::config::{FixedPermission, PermissionConfig, PermissionMode};
    use obsfuse::fs::InodeManager;

    fn create_manager() -> InodeManager {
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

    #[test]
    fn test_directory_hierarchy() {
        let manager = create_manager();

        // Create a directory structure
        let root = 1; // ROOT_INODE
        let dir1 = manager.get_or_create_inode("dir1", true, 0);
        let dir2 = manager.get_or_create_inode("dir1/dir2", true, 0);
        let file = manager.get_or_create_inode("dir1/dir2/file.txt", false, 100);

        // Verify hierarchy
        assert!(manager.get_entry(dir1).unwrap().is_dir);
        assert!(manager.get_entry(dir2).unwrap().is_dir);
        assert!(!manager.get_entry(file).unwrap().is_dir);

        // Verify paths
        assert_eq!(manager.get_path(dir1), Some("dir1".to_string()));
        assert_eq!(manager.get_path(dir2), Some("dir1/dir2".to_string()));
        assert_eq!(manager.get_path(file), Some("dir1/dir2/file.txt".to_string()));
    }

    #[test]
    fn test_rename_preserves_inode() {
        let manager = create_manager();

        let inode = manager.get_or_create_inode("old_name.txt", false, 100);
        let original_attr = manager.get_entry(inode).unwrap().attr.clone();

        manager.rename("old_name.txt", "new_name.txt");

        // Same inode
        assert_eq!(manager.get_inode("new_name.txt"), Some(inode));
        // Same attributes
        let new_attr = manager.get_entry(inode).unwrap().attr;
        assert_eq!(new_attr.size, original_attr.size);
    }
}

#[cfg(test)]
mod cache_integration_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::Bytes;
    use bytesize::ByteSize;

    use obsfuse::cache::{DataCache, MetadataCache, ReadaheadManager, ReadaheadConfig};
    use obsfuse::config::{CacheConfig, MetadataCacheConfig};
    use obsfuse::fs::inode::FileAttr;
    use obsfuse::utils::Metrics;

    #[test]
    fn test_metadata_cache_workflow() {
        let config = MetadataCacheConfig {
            attr_ttl: Duration::from_secs(60),
            dir_ttl: Duration::from_secs(60),
            negative_ttl: Duration::from_secs(30),
            max_entries: 10000,
        };
        let cache = MetadataCache::new(config, Arc::new(Metrics::new()));

        // Simulate file creation
        let attr = FileAttr::file(42, 1024, 1000, 1000, 0o644);
        cache.put_attr(42, attr);

        // Check it's cached
        assert!(cache.get_attr(42).is_some());

        // Simulate modification
        cache.update_attr(42, |a| {
            a.size = 2048;
        });
        assert_eq!(cache.get_attr(42).unwrap().size, 2048);

        // Simulate deletion
        cache.invalidate_attr(42);
        assert!(cache.get_attr(42).is_none());
    }

    #[test]
    fn test_data_cache_large_file() {
        let config = CacheConfig {
            memory_limit: ByteSize::mb(10),
            disk_limit: ByteSize::mb(0),
            cache_dir: None,
            block_size: ByteSize::mb(1),
            metadata: Default::default(),
        };
        let cache = DataCache::new(&config, Arc::new(Metrics::new()));

        // Simulate reading a large file in blocks
        for i in 0..5 {
            let data = Bytes::from(vec![i as u8; 1024 * 1024]); // 1MB blocks
            cache.put(1, i * 1024 * 1024, data);
        }

        // Verify all blocks are cached
        for i in 0..5 {
            let cached = cache.get(1, i * 1024 * 1024);
            assert!(cached.is_some());
            assert_eq!(cached.unwrap()[0], i as u8);
        }
    }

    #[test]
    fn test_readahead_sequential_pattern() {
        let config = ReadaheadConfig {
            enable: true,
            window_size: 4 * 1024 * 1024,
            concurrency: 4,
            seq_threshold: 3,
            prefetch_ttl: Duration::from_secs(30),
        };
        let manager = ReadaheadManager::new(config, Arc::new(Metrics::new()));

        // Simulate sequential reads
        let mut prefetch_triggered = false;
        for i in 0..10 {
            let request = manager.record_read(1, i * 4096, 4096);
            if request.is_some() {
                prefetch_triggered = true;
                // Verify prefetch request is for the next block
                assert!(request.unwrap().offset > i * 4096);
            }
        }

        // Prefetch should have been triggered
        assert!(prefetch_triggered);
    }
}

#[cfg(test)]
mod handle_integration_tests {
    use obsfuse::fs::HandleManager;

    #[test]
    fn test_multiple_handles_same_file() {
        let manager = HandleManager::new();

        // Open the same file multiple times
        let h1 = manager.open(1, libc::O_RDONLY as u32, false);
        let h2 = manager.open(1, libc::O_RDONLY as u32, false);
        let h3 = manager.open(1, libc::O_RDWR as u32, false);

        // All handles should be valid
        assert!(manager.get(h1).is_some());
        assert!(manager.get(h2).is_some());
        assert!(manager.get(h3).is_some());

        // Check handle counts
        assert_eq!(manager.handles_for_inode(1).len(), 3);
        assert!(manager.has_writable_handles(1));

        // Close one handle
        manager.close(h3);
        assert!(!manager.has_writable_handles(1));
        assert!(manager.has_open_handles(1));

        // Close remaining handles
        manager.close(h1);
        manager.close(h2);
        assert!(!manager.has_open_handles(1));
    }
}

#[cfg(test)]
mod obs_client_tests {
    use super::*;

    #[tokio::test]
    async fn test_obs_client_creation() {
        if skip_if_no_credentials() {
            println!("Skipping test: OBS credentials not available");
            return;
        }

        use obsfuse::storage::ObsClient;

        let config = create_test_config();
        let metrics = Arc::new(obsfuse::Metrics::new());

        let result = ObsClient::new(config.obs, metrics);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_obs_list_bucket_root() {
        if skip_if_no_credentials() {
            println!("Skipping test: OBS credentials not available");
            return;
        }

        use obsfuse::storage::ObsClient;

        let config = create_test_config();
        let metrics = Arc::new(obsfuse::Metrics::new());

        let client = ObsClient::new(config.obs, metrics).unwrap();

        // List root of bucket
        let result = client.list_dir("").await;
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod filesystem_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_obsfs_creation() {
        if skip_if_no_credentials() {
            println!("Skipping test: OBS credentials not available");
            return;
        }

        use obsfuse::ObsFs;

        let config = create_test_config();
        let metrics = Arc::new(obsfuse::Metrics::new());

        let result = ObsFs::new(config, metrics);
        assert!(result.is_ok());
    }
}
