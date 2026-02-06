//! Storage abstraction layer
//!
//! This module provides the interface to object storage (OBS)
//! with support for multipart uploads and retry logic.

pub mod multipart;
pub mod obs;
pub mod retry;

pub use multipart::{MultipartConfig, MultipartUploadState, MultipartUploader, PartInfo, StreamingWriter};
pub use obs::{ObjectMeta, ObjectWriteMeta, ObsClient};
pub use retry::{retry_simple, retry_with_backoff, IsRetryable, RetryConfig};
