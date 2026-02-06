//! Unit tests for OBS FUSE components
//!
//! This module contains comprehensive unit tests for all core components:
//! - Inode management
//! - Cache systems (metadata, data, readahead, write buffer)
//! - File handle management
//! - Storage operations
//! - Configuration
//! - Permission and attribute handling
//! - Directory operations
//! - Error types and metrics

mod test_cache;
mod test_config;
mod test_dir;
mod test_handle;
mod test_inode;
mod test_permission;
mod test_storage;
mod test_utils;
