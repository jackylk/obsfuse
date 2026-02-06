//! Filesystem module for OBS FUSE
//!
//! This module contains the core filesystem implementation including
//! inode management, file handles, and the platform-specific filesystem interfaces.

pub mod attr;
pub mod core;
pub mod dir;
pub mod handle;
pub mod inode;
pub mod permission;
pub mod platform;

// Platform-specific implementations
#[cfg(unix)]
pub mod fuse_impl;
#[cfg(windows)]
pub mod winfsp_impl;

// Re-export common types
pub use handle::{HandleManager, HandleState};
pub use inode::{FileAttr, InodeEntry, InodeManager, ROOT_INODE};
pub use permission::PermissionManager;
pub use platform::FileKind;

// Re-export core
pub use core::ObsFsCore;

// Platform-specific re-exports
#[cfg(unix)]
pub use fuse_impl::ObsFs;

#[cfg(windows)]
pub use winfsp_impl::{WinFspFs, mount_winfsp};
