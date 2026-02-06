//! Retry logic for storage operations
//!
//! This module provides retry utilities with exponential backoff
//! for handling transient failures.

use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

use crate::utils::ObsFuseError;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        }
    }
}

/// Retry an async operation with exponential backoff
pub async fn retry_with_backoff<F, Fut, T, E>(
    config: &RetryConfig,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display + IsRetryable,
{
    let mut attempt = 0;
    let mut delay = config.initial_delay;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;

                if attempt > config.max_retries || !e.is_retryable() {
                    return Err(e);
                }

                warn!(
                    attempt = attempt,
                    max_retries = config.max_retries,
                    delay_ms = delay.as_millis(),
                    error = %e,
                    "Operation failed, retrying"
                );

                sleep(delay).await;

                // Exponential backoff
                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_multiplier).min(config.max_delay.as_secs_f64()),
                );
            }
        }
    }
}

/// Trait for determining if an error is retryable
pub trait IsRetryable {
    fn is_retryable(&self) -> bool;
}

impl IsRetryable for ObsFuseError {
    fn is_retryable(&self) -> bool {
        match self {
            ObsFuseError::Storage(e) => {
                use opendal::ErrorKind;
                matches!(
                    e.kind(),
                    ErrorKind::RateLimited | ErrorKind::Unexpected
                )
            }
            ObsFuseError::Io(e) => {
                use std::io::ErrorKind;
                matches!(
                    e.kind(),
                    ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::TimedOut
                        | ErrorKind::Interrupted
                )
            }
            _ => false,
        }
    }
}

impl IsRetryable for opendal::Error {
    fn is_retryable(&self) -> bool {
        use opendal::ErrorKind;
        matches!(
            self.kind(),
            ErrorKind::RateLimited | ErrorKind::Unexpected
        )
    }
}

/// Simple retry without backoff
pub async fn retry_simple<F, Fut, T, E>(
    max_retries: u32,
    delay: Duration,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display + IsRetryable,
{
    let config = RetryConfig {
        max_retries,
        initial_delay: delay,
        max_delay: delay,
        backoff_multiplier: 1.0,
    };
    retry_with_backoff(&config, operation).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct TestError(bool);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError(retryable={})", self.0)
        }
    }

    impl IsRetryable for TestError {
        fn is_retryable(&self) -> bool {
            self.0
        }
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let config = RetryConfig::default();
        let result: Result<i32, TestError> =
            retry_with_backoff(&config, || async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
        };

        let attempts = AtomicU32::new(0);
        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    Err(TestError(true))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let config = RetryConfig::default();
        let attempts = AtomicU32::new(0);

        let result: Result<i32, TestError> = retry_with_backoff(&config, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(TestError(false)) }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
