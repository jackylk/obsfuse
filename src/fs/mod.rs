//! Filesystem module for OBS FUSE
//!
//! This module contains the core filesystem implementation including
//! inode management, file handles, and the FUSE interface.

pub mod attr;
pub mod dir;
pub mod handle;
pub mod inode;
pub mod obsfs;
pub mod permission;

pub use inode::{FileAttr, InodeEntry, InodeManager, ROOT_INODE};
pub use handle::{HandleManager, HandleState};
pub use obsfs::ObsFs;
pub use permission::PermissionManager;
