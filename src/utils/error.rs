//! Error types for OBS FUSE filesystem
//!
//! This module defines custom error types that map to appropriate
//! FUSE error codes for proper error handling.

use std::io;
use thiserror::Error;

/// Main error type for OBS FUSE operations
#[derive(Error, Debug)]
pub enum ObsFuseError {
    /// I/O error from underlying operations
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// OBS/OpenDAL storage errors
    #[error("Storage error: {0}")]
    Storage(#[from] opendal::Error),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Inode not found
    #[error("Inode not found: {0}")]
    InodeNotFound(u64),

    /// Path not found
    #[error("Path not found: {0}")]
    PathNotFound(String),

    /// File handle not found
    #[error("File handle not found: {0}")]
    HandleNotFound(u64),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// File exists
    #[error("File exists: {0}")]
    FileExists(String),

    /// Not a directory
    #[error("Not a directory: {0}")]
    NotADirectory(String),

    /// Is a directory
    #[error("Is a directory: {0}")]
    IsADirectory(String),

    /// Directory not empty
    #[error("Directory not empty: {0}")]
    DirectoryNotEmpty(String),

    /// Invalid argument
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Operation not supported
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Cache error
    #[error("Cache error: {0}")]
    Cache(String),

    /// Multipart upload error
    #[error("Multipart upload error: {0}")]
    MultipartUpload(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl ObsFuseError {
    /// Convert error to libc errno code
    pub fn to_errno(&self) -> libc::c_int {
        match self {
            ObsFuseError::Io(e) => e.raw_os_error().unwrap_or(libc::EIO),
            ObsFuseError::Storage(e) => storage_error_to_errno(e),
            ObsFuseError::Config(_) => libc::EINVAL,
            ObsFuseError::InodeNotFound(_) => libc::ENOENT,
            ObsFuseError::PathNotFound(_) => libc::ENOENT,
            ObsFuseError::HandleNotFound(_) => libc::EBADF,
            ObsFuseError::PermissionDenied(_) => libc::EACCES,
            ObsFuseError::FileExists(_) => libc::EEXIST,
            ObsFuseError::NotADirectory(_) => libc::ENOTDIR,
            ObsFuseError::IsADirectory(_) => libc::EISDIR,
            ObsFuseError::DirectoryNotEmpty(_) => libc::ENOTEMPTY,
            ObsFuseError::InvalidArgument(_) => libc::EINVAL,
            ObsFuseError::NotSupported(_) => libc::ENOSYS,
            ObsFuseError::Cache(_) => libc::EIO,
            ObsFuseError::MultipartUpload(_) => libc::EIO,
            ObsFuseError::Internal(_) => libc::EIO,
        }
    }
}

/// Convert OpenDAL error to errno
fn storage_error_to_errno(e: &opendal::Error) -> libc::c_int {
    use opendal::ErrorKind;

    match e.kind() {
        ErrorKind::NotFound => libc::ENOENT,
        ErrorKind::PermissionDenied => libc::EACCES,
        ErrorKind::AlreadyExists => libc::EEXIST,
        ErrorKind::NotADirectory => libc::ENOTDIR,
        ErrorKind::IsADirectory => libc::EISDIR,
        ErrorKind::RateLimited => libc::EAGAIN,
        ErrorKind::Unsupported => libc::ENOSYS,
        _ => libc::EIO,
    }
}

/// Result type alias for OBS FUSE operations
pub type Result<T> = std::result::Result<T, ObsFuseError>;

/// Extension trait for converting errors to io::Error
pub trait ToIoError {
    fn to_io_error(&self) -> io::Error;
}

impl ToIoError for ObsFuseError {
    fn to_io_error(&self) -> io::Error {
        io::Error::from_raw_os_error(self.to_errno())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_to_errno() {
        let err = ObsFuseError::PathNotFound("/test".to_string());
        assert_eq!(err.to_errno(), libc::ENOENT);

        let err = ObsFuseError::PermissionDenied("test".to_string());
        assert_eq!(err.to_errno(), libc::EACCES);

        let err = ObsFuseError::FileExists("/test".to_string());
        assert_eq!(err.to_errno(), libc::EEXIST);
    }
}
