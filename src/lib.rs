//! OBS FUSE - High-performance FUSE filesystem for Huawei Cloud OBS
//!
//! This crate provides a FUSE filesystem that mounts Huawei Cloud OBS
//! (Object Storage Service) as a local filesystem.
//!
//! # Features
//!
//! - **High Performance**: Multi-level caching, read-ahead, and write buffering
//! - **Strong Consistency**: Write-through caching with short TTLs
//! - **Cross-Platform**: Supports Linux and macOS
//! - **Configurable**: Flexible permission modes and cache settings
//!
//! # Example
//!
//! ```no_run
//! use obsfuse::{Config, ObsFs};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = Config::default();
//!     let metrics = Arc::new(obsfuse::Metrics::new());
//!     let fs = ObsFs::new(config, metrics)?;
//!     // Mount the filesystem...
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
pub use fs::ObsFs;
pub use utils::{Metrics, ObsFuseError, Result};
