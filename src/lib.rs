//! OBS FUSE - High-performance FUSE filesystem for Huawei Cloud OBS
//!
//! This crate provides a FUSE filesystem that mounts Huawei Cloud OBS
//! (Object Storage Service) as a local filesystem.
//!
//! # Features
//!
//! - **High Performance**: Multi-level caching, read-ahead, and write buffering
//! - **Strong Consistency**: Write-through caching with short TTLs
//! - **Cross-Platform**: Supports Linux, macOS (via FUSE), and Windows (via WinFSP)
//! - **Configurable**: Flexible permission modes and cache settings
//!
//! # Platform Support
//!
//! - **Unix (Linux, macOS)**: Uses FUSE3 for kernel-level filesystem integration
//! - **Windows**: Uses WinFSP for Windows filesystem integration
//!
//! # Example
//!
//! ```no_run
//! use obsfuse::{Config, ObsFsCore};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = Config::default();
//!     let metrics = Arc::new(obsfuse::Metrics::new());
//!     let core = ObsFsCore::new(config, metrics)?;
//!     // Mount the filesystem using platform-specific adapters...
//!     Ok(())
//! }
//! ```

pub mod cache;
pub mod config;
pub mod fs;
pub mod storage;
pub mod utils;

// Re-exports for convenience
pub use config::Config;
pub use fs::ObsFsCore;
pub use utils::{Metrics, ObsFuseError, Result};

// Platform-specific re-exports
#[cfg(unix)]
pub use fs::ObsFs;

#[cfg(windows)]
pub use fs::{WinFspFs, mount_winfsp};
