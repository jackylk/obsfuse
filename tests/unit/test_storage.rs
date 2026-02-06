//! Comprehensive unit tests for Storage module
//!
//! Tests cover:
//! - Retry logic
//! - Multipart upload state
//! - Object metadata

#[cfg(test)]
mod retry_tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use obsfuse::storage::{IsRetryable, RetryConfig};

    // Test error type
    #[derive(Debug)]
    struct TestError {
        retryable: bool,
        message: String,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl IsRetryable for TestError {
        fn is_retryable(&self) -> bool {
            self.retryable
        }
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();

        assert_eq!(config.max_retries, 3);
        assert!(config.initial_delay.as_millis() > 0);
        assert!(config.max_delay.as_secs() > 0);
        assert!(config.backoff_multiplier > 1.0);
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        use obsfuse::storage::retry_with_backoff;

        let config = RetryConfig::default();
        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Ok(42) }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        use obsfuse::storage::retry_with_backoff;

        let config = RetryConfig {
            max_retries: 5,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
        };

        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 3 {
                    Err(TestError {
                        retryable: true,
                        message: "temporary failure".to_string(),
                    })
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        use obsfuse::storage::retry_with_backoff;

        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
        };

        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                Err(TestError {
                    retryable: true,
                    message: "always fails".to_string(),
                })
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 4); // Initial + 3 retries
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        use obsfuse::storage::retry_with_backoff;

        let config = RetryConfig::default();
        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                Err(TestError {
                    retryable: false,
                    message: "permanent failure".to_string(),
                })
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1); // No retries
    }

    #[tokio::test]
    async fn test_retry_simple() {
        use obsfuse::storage::retry_simple;

        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_simple(
            3,
            Duration::from_millis(1),
            || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                async move {
                    if attempt < 2 {
                        Err(TestError {
                            retryable: true,
                            message: "fail".to_string(),
                        })
                    } else {
                        Ok(99)
                    }
                }
            },
        )
        .await;

        assert_eq!(result.unwrap(), 99);
    }
}

#[cfg(test)]
mod multipart_tests {
    use obsfuse::storage::{MultipartConfig, MultipartUploadState, PartInfo};

    #[test]
    fn test_multipart_config_default() {
        let config = MultipartConfig::default();

        assert_eq!(config.part_size, 8 * 1024 * 1024);
        assert_eq!(config.concurrency, 5);
        assert_eq!(config.max_parts, 10000);
    }

    #[test]
    fn test_multipart_state_creation() {
        let state = MultipartUploadState::new("path/to/file.txt".to_string());

        assert_eq!(state.path, "path/to/file.txt");
        assert!(state.parts.is_empty());
        assert_eq!(state.current_part, 1);
        assert!(state.buffer.is_empty());
        assert_eq!(state.total_bytes, 0);
        assert!(!state.initiated);
    }

    #[test]
    fn test_multipart_state_buffer_ready() {
        let mut state = MultipartUploadState::new("test.txt".to_string());

        assert!(!state.buffer_ready(1024));

        state.buffer.extend_from_slice(&[0u8; 500]);
        assert!(!state.buffer_ready(1024));

        state.buffer.extend_from_slice(&[0u8; 600]);
        assert!(state.buffer_ready(1024));
    }

    #[test]
    fn test_multipart_state_take_buffer() {
        let mut state = MultipartUploadState::new("test.txt".to_string());

        state.buffer.extend_from_slice(&[1u8; 2048]);

        let taken = state.take_buffer(1024);
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().len(), 1024);
        assert_eq!(state.buffer.len(), 1024);

        let taken = state.take_buffer(1024);
        assert!(taken.is_some());
        assert!(state.buffer.is_empty());

        let taken = state.take_buffer(1024);
        assert!(taken.is_none());
    }

    #[test]
    fn test_multipart_state_take_remaining() {
        let mut state = MultipartUploadState::new("test.txt".to_string());

        state.buffer.extend_from_slice(&[42u8; 500]);

        let remaining = state.take_remaining();
        assert_eq!(remaining.len(), 500);
        assert!(state.buffer.is_empty());
    }

    #[test]
    fn test_part_info() {
        let part = PartInfo {
            part_number: 1,
            etag: "abc123".to_string(),
            size: 8 * 1024 * 1024,
        };

        assert_eq!(part.part_number, 1);
        assert_eq!(part.etag, "abc123");
        assert_eq!(part.size, 8 * 1024 * 1024);
    }
}

#[cfg(test)]
mod object_meta_tests {
    use std::time::SystemTime;

    use obsfuse::storage::ObjectMeta;

    #[test]
    fn test_object_meta_name_from_path() {
        let meta = ObjectMeta {
            path: "prefix/dir/file.txt".to_string(),
            size: 100,
            last_modified: None,
            is_dir: false,
            content_type: None,
            etag: None,
        };

        assert_eq!(meta.name(), "file.txt");
    }

    #[test]
    fn test_object_meta_name_from_dir() {
        let meta = ObjectMeta {
            path: "prefix/dir/".to_string(),
            size: 0,
            last_modified: None,
            is_dir: true,
            content_type: None,
            etag: None,
        };

        assert_eq!(meta.name(), "dir");
    }

    #[test]
    fn test_object_meta_name_root() {
        let meta = ObjectMeta {
            path: "file.txt".to_string(),
            size: 100,
            last_modified: None,
            is_dir: false,
            content_type: None,
            etag: None,
        };

        assert_eq!(meta.name(), "file.txt");
    }

    #[test]
    fn test_object_meta_fields() {
        let now = SystemTime::now();
        let meta = ObjectMeta {
            path: "path/file.txt".to_string(),
            size: 1024,
            last_modified: Some(now),
            is_dir: false,
            content_type: Some("text/plain".to_string()),
            etag: Some("abc123".to_string()),
        };

        assert_eq!(meta.size, 1024);
        assert!(!meta.is_dir);
        assert_eq!(meta.content_type, Some("text/plain".to_string()));
        assert_eq!(meta.etag, Some("abc123".to_string()));
    }
}
