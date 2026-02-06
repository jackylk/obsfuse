//! Comprehensive unit tests for Error types and Metrics
//!
//! Tests cover:
//! - Error type creation
//! - Error to errno conversion
//! - Metrics collection
//! - Metrics statistics

#[cfg(test)]
mod error_tests {
    use std::io;

    use obsfuse::utils::{ObsFuseError, ToIoError};

    // ==================== Error Creation ====================

    #[test]
    fn test_error_inode_not_found() {
        let err = ObsFuseError::InodeNotFound(42);
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn test_error_path_not_found() {
        let err = ObsFuseError::PathNotFound("/test/path".to_string());
        assert!(err.to_string().contains("/test/path"));
    }

    #[test]
    fn test_error_handle_not_found() {
        let err = ObsFuseError::HandleNotFound(123);
        assert!(err.to_string().contains("123"));
    }

    #[test]
    fn test_error_permission_denied() {
        let err = ObsFuseError::PermissionDenied("test operation".to_string());
        assert!(err.to_string().contains("test operation"));
    }

    #[test]
    fn test_error_file_exists() {
        let err = ObsFuseError::FileExists("/existing/file".to_string());
        assert!(err.to_string().contains("/existing/file"));
    }

    #[test]
    fn test_error_not_a_directory() {
        let err = ObsFuseError::NotADirectory("/not/a/dir".to_string());
        assert!(err.to_string().contains("/not/a/dir"));
    }

    #[test]
    fn test_error_is_a_directory() {
        let err = ObsFuseError::IsADirectory("/is/a/dir".to_string());
        assert!(err.to_string().contains("/is/a/dir"));
    }

    #[test]
    fn test_error_directory_not_empty() {
        let err = ObsFuseError::DirectoryNotEmpty("/non/empty/dir".to_string());
        assert!(err.to_string().contains("/non/empty/dir"));
    }

    #[test]
    fn test_error_invalid_argument() {
        let err = ObsFuseError::InvalidArgument("bad arg".to_string());
        assert!(err.to_string().contains("bad arg"));
    }

    #[test]
    fn test_error_not_supported() {
        let err = ObsFuseError::NotSupported("xattr".to_string());
        assert!(err.to_string().contains("xattr"));
    }

    #[test]
    fn test_error_config() {
        let err = ObsFuseError::Config("invalid config".to_string());
        assert!(err.to_string().contains("invalid config"));
    }

    #[test]
    fn test_error_cache() {
        let err = ObsFuseError::Cache("cache error".to_string());
        assert!(err.to_string().contains("cache error"));
    }

    #[test]
    fn test_error_multipart_upload() {
        let err = ObsFuseError::MultipartUpload("upload failed".to_string());
        assert!(err.to_string().contains("upload failed"));
    }

    #[test]
    fn test_error_internal() {
        let err = ObsFuseError::Internal("internal error".to_string());
        assert!(err.to_string().contains("internal error"));
    }

    // ==================== Error to Errno ====================

    #[test]
    fn test_to_errno_inode_not_found() {
        let err = ObsFuseError::InodeNotFound(1);
        assert_eq!(err.to_errno(), libc::ENOENT);
    }

    #[test]
    fn test_to_errno_path_not_found() {
        let err = ObsFuseError::PathNotFound("/path".to_string());
        assert_eq!(err.to_errno(), libc::ENOENT);
    }

    #[test]
    fn test_to_errno_handle_not_found() {
        let err = ObsFuseError::HandleNotFound(1);
        assert_eq!(err.to_errno(), libc::EBADF);
    }

    #[test]
    fn test_to_errno_permission_denied() {
        let err = ObsFuseError::PermissionDenied("test".to_string());
        assert_eq!(err.to_errno(), libc::EACCES);
    }

    #[test]
    fn test_to_errno_file_exists() {
        let err = ObsFuseError::FileExists("/file".to_string());
        assert_eq!(err.to_errno(), libc::EEXIST);
    }

    #[test]
    fn test_to_errno_not_a_directory() {
        let err = ObsFuseError::NotADirectory("/path".to_string());
        assert_eq!(err.to_errno(), libc::ENOTDIR);
    }

    #[test]
    fn test_to_errno_is_a_directory() {
        let err = ObsFuseError::IsADirectory("/dir".to_string());
        assert_eq!(err.to_errno(), libc::EISDIR);
    }

    #[test]
    fn test_to_errno_directory_not_empty() {
        let err = ObsFuseError::DirectoryNotEmpty("/dir".to_string());
        assert_eq!(err.to_errno(), libc::ENOTEMPTY);
    }

    #[test]
    fn test_to_errno_invalid_argument() {
        let err = ObsFuseError::InvalidArgument("arg".to_string());
        assert_eq!(err.to_errno(), libc::EINVAL);
    }

    #[test]
    fn test_to_errno_not_supported() {
        let err = ObsFuseError::NotSupported("op".to_string());
        assert_eq!(err.to_errno(), libc::ENOSYS);
    }

    #[test]
    fn test_to_errno_config() {
        let err = ObsFuseError::Config("config".to_string());
        assert_eq!(err.to_errno(), libc::EINVAL);
    }

    #[test]
    fn test_to_errno_cache() {
        let err = ObsFuseError::Cache("cache".to_string());
        assert_eq!(err.to_errno(), libc::EIO);
    }

    #[test]
    fn test_to_errno_multipart() {
        let err = ObsFuseError::MultipartUpload("mp".to_string());
        assert_eq!(err.to_errno(), libc::EIO);
    }

    #[test]
    fn test_to_errno_internal() {
        let err = ObsFuseError::Internal("internal".to_string());
        assert_eq!(err.to_errno(), libc::EIO);
    }

    #[test]
    fn test_to_errno_io() {
        let err = ObsFuseError::Io(io::Error::from_raw_os_error(libc::EPERM));
        assert_eq!(err.to_errno(), libc::EPERM);
    }

    // ==================== ToIoError ====================

    #[test]
    fn test_to_io_error() {
        let err = ObsFuseError::PathNotFound("/path".to_string());
        let io_err = err.to_io_error();

        assert_eq!(io_err.raw_os_error(), Some(libc::ENOENT));
    }

    // ==================== From Implementations ====================

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::from_raw_os_error(libc::ENOENT);
        let err: ObsFuseError = io_err.into();

        assert!(matches!(err, ObsFuseError::Io(_)));
        assert_eq!(err.to_errno(), libc::ENOENT);
    }
}

#[cfg(test)]
mod metrics_tests {
    use std::time::Duration;

    use obsfuse::utils::Metrics;

    // ==================== Metrics Creation ====================

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        let summary = metrics.summary();

        assert_eq!(summary.read_ops, 0);
        assert_eq!(summary.write_ops, 0);
        assert_eq!(summary.obs_get_ops, 0);
    }

    // ==================== Read Operations ====================

    #[test]
    fn test_inc_read_ops() {
        let metrics = Metrics::new();

        metrics.inc_read_ops();
        metrics.inc_read_ops();
        metrics.inc_read_ops();

        assert_eq!(metrics.summary().read_ops, 3);
    }

    #[test]
    fn test_add_read_bytes() {
        let metrics = Metrics::new();

        metrics.add_read_bytes(1024);
        metrics.add_read_bytes(2048);

        assert_eq!(metrics.summary().read_bytes, 3072);
    }

    #[test]
    fn test_read_cache_hit_miss() {
        let metrics = Metrics::new();

        metrics.inc_read_cache_hit();
        metrics.inc_read_cache_hit();
        metrics.inc_read_cache_miss();

        let summary = metrics.summary();
        assert_eq!(summary.read_cache_hit_rate, 2.0 / 3.0);
    }

    #[test]
    fn test_read_cache_hit_rate_zero() {
        let metrics = Metrics::new();
        assert_eq!(metrics.read_cache_hit_rate(), 0.0);
    }

    // ==================== Write Operations ====================

    #[test]
    fn test_inc_write_ops() {
        let metrics = Metrics::new();

        metrics.inc_write_ops();
        metrics.inc_write_ops();

        assert_eq!(metrics.summary().write_ops, 2);
    }

    #[test]
    fn test_add_write_bytes() {
        let metrics = Metrics::new();

        metrics.add_write_bytes(512);
        metrics.add_write_bytes(512);

        assert_eq!(metrics.summary().write_bytes, 1024);
    }

    #[test]
    fn test_inc_write_buffer_flush() {
        let metrics = Metrics::new();

        metrics.inc_write_buffer_flush();
        metrics.inc_write_buffer_flush();
        metrics.inc_write_buffer_flush();

        assert_eq!(metrics.summary().write_buffer_flushes, 3);
    }

    // ==================== Metadata Operations ====================

    #[test]
    fn test_inc_lookup_ops() {
        let metrics = Metrics::new();

        metrics.inc_lookup_ops();

        assert_eq!(metrics.summary().lookup_ops, 1);
    }

    #[test]
    fn test_inc_getattr_ops() {
        let metrics = Metrics::new();

        metrics.inc_getattr_ops();
        metrics.inc_getattr_ops();

        assert_eq!(metrics.summary().getattr_ops, 2);
    }

    #[test]
    fn test_inc_readdir_ops() {
        let metrics = Metrics::new();

        metrics.inc_readdir_ops();

        assert_eq!(metrics.summary().readdir_ops, 1);
    }

    #[test]
    fn test_metadata_cache_hit_miss() {
        let metrics = Metrics::new();

        metrics.inc_metadata_cache_hit();
        metrics.inc_metadata_cache_hit();
        metrics.inc_metadata_cache_hit();
        metrics.inc_metadata_cache_miss();

        assert_eq!(metrics.metadata_cache_hit_rate(), 0.75);
    }

    // ==================== OBS Operations ====================

    #[test]
    fn test_inc_obs_get() {
        let metrics = Metrics::new();

        metrics.inc_obs_get();
        metrics.inc_obs_get();

        assert_eq!(metrics.summary().obs_get_ops, 2);
    }

    #[test]
    fn test_inc_obs_put() {
        let metrics = Metrics::new();

        metrics.inc_obs_put();

        assert_eq!(metrics.summary().obs_put_ops, 1);
    }

    #[test]
    fn test_inc_obs_list() {
        let metrics = Metrics::new();

        metrics.inc_obs_list();
        metrics.inc_obs_list();
        metrics.inc_obs_list();

        assert_eq!(metrics.summary().obs_list_ops, 3);
    }

    #[test]
    fn test_inc_obs_delete() {
        let metrics = Metrics::new();

        metrics.inc_obs_delete();

        assert_eq!(metrics.summary().obs_delete_ops, 1);
    }

    #[test]
    fn test_inc_obs_error() {
        let metrics = Metrics::new();

        metrics.inc_obs_error();
        metrics.inc_obs_error();

        assert_eq!(metrics.summary().obs_errors, 2);
    }

    // ==================== Uptime ====================

    #[test]
    fn test_uptime() {
        let metrics = Metrics::new();

        std::thread::sleep(Duration::from_millis(10));

        let uptime = metrics.uptime();
        assert!(uptime.as_millis() >= 10);
    }

    // ==================== Summary ====================

    #[test]
    fn test_summary() {
        let metrics = Metrics::new();

        metrics.inc_read_ops();
        metrics.add_read_bytes(1024);
        metrics.inc_write_ops();
        metrics.add_write_bytes(512);
        metrics.inc_obs_get();
        metrics.inc_obs_put();
        metrics.inc_obs_error();

        let summary = metrics.summary();

        assert_eq!(summary.read_ops, 1);
        assert_eq!(summary.read_bytes, 1024);
        assert_eq!(summary.write_ops, 1);
        assert_eq!(summary.write_bytes, 512);
        assert_eq!(summary.obs_get_ops, 1);
        assert_eq!(summary.obs_put_ops, 1);
        assert_eq!(summary.obs_errors, 1);
    }

    #[test]
    fn test_summary_display() {
        let metrics = Metrics::new();

        metrics.inc_read_ops();
        metrics.add_read_bytes(1024 * 1024); // 1MB

        let summary = metrics.summary();
        let display = format!("{}", summary);

        assert!(display.contains("Read Operations"));
        assert!(display.contains("1 MB"));
    }

    // ==================== Concurrent Updates ====================

    #[test]
    fn test_concurrent_metrics() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(Metrics::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let metrics = Arc::clone(&metrics);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    metrics.inc_read_ops();
                    metrics.inc_write_ops();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let summary = metrics.summary();
        assert_eq!(summary.read_ops, 1000);
        assert_eq!(summary.write_ops, 1000);
    }
}
