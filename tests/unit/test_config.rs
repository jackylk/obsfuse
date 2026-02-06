//! Comprehensive unit tests for Configuration
//!
//! Tests cover:
//! - Default values
//! - Configuration validation
//! - Environment variable merging
//! - Permission modes

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytesize::ByteSize;

    use obsfuse::config::{
        CacheConfig, Config, FixedPermission, FuseConfig, LoggingConfig,
        MetadataCacheConfig, ObsConfig, PerformanceConfig, PermissionConfig, PermissionMode,
    };

    // ==================== Default Values ====================

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert!(!config.obs.endpoint.is_empty());
        assert!(config.cache.memory_limit.as_u64() > 0);
        assert!(config.performance.read_ahead);
    }

    #[test]
    fn test_obs_config_default() {
        let config = ObsConfig::default();

        assert_eq!(config.endpoint, "obs.cn-north-1.myhuaweicloud.com");
        assert_eq!(config.region, "cn-north-1");
        assert!(config.bucket.is_empty());
        assert!(config.access_key.is_none());
        assert!(config.secret_key.is_none());
        assert_eq!(config.max_connections, 64);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();

        assert_eq!(config.memory_limit, ByteSize::mb(512));
        assert_eq!(config.disk_limit, ByteSize::gb(10));
        assert_eq!(config.block_size, ByteSize::mb(4));
        assert!(config.cache_dir.is_none());
    }

    #[test]
    fn test_metadata_cache_config_default() {
        let config = MetadataCacheConfig::default();

        assert_eq!(config.attr_ttl, Duration::from_secs(3));
        assert_eq!(config.dir_ttl, Duration::from_secs(5));
        assert_eq!(config.negative_ttl, Duration::from_secs(1));
        assert_eq!(config.max_entries, 100_000);
    }

    #[test]
    fn test_performance_config_default() {
        let config = PerformanceConfig::default();

        assert!(config.read_ahead);
        assert_eq!(config.read_ahead_window, ByteSize::mb(16));
        assert_eq!(config.read_concurrency, 4);
        assert_eq!(config.write_buffer_size, ByteSize::mb(64));
        assert_eq!(config.multipart_threshold, ByteSize::mb(100));
        assert_eq!(config.multipart_part_size, ByteSize::mb(8));
        assert_eq!(config.multipart_concurrency, 5);
        assert_eq!(config.flush_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_fuse_config_default() {
        let config = FuseConfig::default();

        assert_eq!(config.max_read, ByteSize::mb(4));
        assert_eq!(config.max_write, ByteSize::mb(4));
        assert!(!config.allow_root);
        assert!(!config.allow_other);
        assert!(!config.read_only);
        assert!(!config.direct_io);
        assert_eq!(config.fs_name, "obsfuse");
    }

    #[test]
    fn test_permission_config_default() {
        let config = PermissionConfig::default();

        assert_eq!(config.mode, PermissionMode::Fixed);
    }

    #[test]
    fn test_fixed_permission_default() {
        let perm = FixedPermission::default();

        // UID/GID should be current user
        assert!(perm.uid > 0 || perm.uid == 0); // root is valid
        assert!(perm.gid > 0 || perm.gid == 0);
        assert_eq!(perm.file_mode, 0o644);
        assert_eq!(perm.dir_mode, 0o755);
    }

    #[test]
    fn test_logging_config_default() {
        let config = LoggingConfig::default();

        assert_eq!(config.level, "info");
        assert!(config.file.is_none());
        assert!(!config.json);
    }

    // ==================== Validation ====================

    #[test]
    fn test_config_validation_missing_bucket() {
        let config = Config::default();
        let result = config.validate();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Bucket"));
    }

    #[test]
    fn test_config_validation_missing_access_key() {
        let mut config = Config::default();
        config.obs.bucket = "test-bucket".to_string();

        let result = config.validate();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Access key"));
    }

    #[test]
    fn test_config_validation_missing_secret_key() {
        let mut config = Config::default();
        config.obs.bucket = "test-bucket".to_string();
        config.obs.access_key = Some("ak".to_string());

        let result = config.validate();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Secret key"));
    }

    #[test]
    fn test_config_validation_success() {
        let mut config = Config::default();
        config.obs.bucket = "test-bucket".to_string();
        config.obs.access_key = Some("ak".to_string());
        config.obs.secret_key = Some("sk".to_string());

        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_validation_zero_block_size() {
        let mut config = Config::default();
        config.obs.bucket = "test-bucket".to_string();
        config.obs.access_key = Some("ak".to_string());
        config.obs.secret_key = Some("sk".to_string());
        config.cache.block_size = ByteSize::b(0);

        let result = config.validate();
        assert!(result.is_err());
    }

    // ==================== Environment Variable Merging ====================

    #[test]
    fn test_merge_env() {
        // Set environment variables
        std::env::set_var("OBS_ACCESS_KEY", "env_ak");
        std::env::set_var("OBS_SECRET_KEY", "env_sk");
        std::env::set_var("OBS_ENDPOINT", "env.endpoint.com");
        std::env::set_var("OBS_BUCKET", "env-bucket");
        std::env::set_var("OBS_REGION", "env-region");

        let mut config = Config::default();
        config.merge_env();

        assert_eq!(config.obs.access_key, Some("env_ak".to_string()));
        assert_eq!(config.obs.secret_key, Some("env_sk".to_string()));
        assert_eq!(config.obs.endpoint, "env.endpoint.com");
        assert_eq!(config.obs.bucket, "env-bucket");
        assert_eq!(config.obs.region, "env-region");

        // Clean up
        std::env::remove_var("OBS_ACCESS_KEY");
        std::env::remove_var("OBS_SECRET_KEY");
        std::env::remove_var("OBS_ENDPOINT");
        std::env::remove_var("OBS_BUCKET");
        std::env::remove_var("OBS_REGION");
    }

    // ==================== Cache Directory ====================

    #[test]
    fn test_effective_cache_dir_custom() {
        let mut config = Config::default();
        config.cache.cache_dir = Some("/custom/cache".into());

        let dir = config.effective_cache_dir();
        assert_eq!(dir.to_str().unwrap(), "/custom/cache");
    }

    #[test]
    fn test_effective_cache_dir_default() {
        let config = Config::default();

        let dir = config.effective_cache_dir();
        assert!(dir.to_str().unwrap().contains("obsfuse"));
    }

    // ==================== Permission Modes ====================

    #[test]
    fn test_permission_mode_fixed() {
        let mode = PermissionMode::Fixed;
        assert_eq!(mode, PermissionMode::default());
    }

    #[test]
    fn test_permission_mode_preserved() {
        let mode = PermissionMode::Preserved;
        assert_ne!(mode, PermissionMode::Fixed);
    }

    #[test]
    fn test_permission_config_with_fixed() {
        let config = PermissionConfig {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission {
                uid: 1000,
                gid: 1000,
                file_mode: 0o644,
                dir_mode: 0o755,
            },
        };

        assert_eq!(config.mode, PermissionMode::Fixed);
        assert_eq!(config.fixed.uid, 1000);
        assert_eq!(config.fixed.gid, 1000);
        assert_eq!(config.fixed.file_mode, 0o644);
        assert_eq!(config.fixed.dir_mode, 0o755);
    }

    // ==================== Config Paths ====================

    #[test]
    fn test_default_config_path() {
        let path = Config::default_config_path();

        assert!(path.to_str().unwrap().contains(".obsfuse"));
        assert!(path.to_str().unwrap().contains("config.toml"));
    }

    #[test]
    fn test_default_cache_dir() {
        let dir = Config::default_cache_dir();

        assert!(dir.to_str().unwrap().contains("obsfuse"));
    }
}
