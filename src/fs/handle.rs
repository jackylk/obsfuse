//! File handle management for OBS FUSE filesystem
//!
//! This module manages open file handles and their associated state.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::debug;

/// File handle manager
pub struct HandleManager {
    /// Handle to state mapping
    handles: DashMap<u64, HandleState>,
    /// Next handle number
    next_handle: AtomicU64,
}

/// State for an open file handle
#[derive(Debug, Clone)]
pub struct HandleState {
    /// Associated inode
    pub inode: u64,
    /// Open flags
    pub flags: u32,
    /// Whether file is opened for writing
    pub writable: bool,
    /// Whether file is opened for reading
    pub readable: bool,
    /// Current file position (for sequential access detection)
    pub position: u64,
    /// Last access time
    pub last_access: Instant,
    /// Whether data has been modified
    pub dirty: bool,
    /// Whether this is a directory handle
    pub is_dir: bool,
}

impl HandleState {
    /// Create a new handle state
    pub fn new(inode: u64, flags: u32, is_dir: bool) -> Self {
        let writable = Self::is_writable(flags);
        let readable = Self::is_readable(flags);

        Self {
            inode,
            flags,
            writable,
            readable,
            position: 0,
            last_access: Instant::now(),
            dirty: false,
            is_dir,
        }
    }

    /// Check if flags indicate writable
    fn is_writable(flags: u32) -> bool {
        let access_mode = flags & libc::O_ACCMODE as u32;
        access_mode == libc::O_WRONLY as u32 || access_mode == libc::O_RDWR as u32
    }

    /// Check if flags indicate readable
    fn is_readable(flags: u32) -> bool {
        let access_mode = flags & libc::O_ACCMODE as u32;
        access_mode == libc::O_RDONLY as u32 || access_mode == libc::O_RDWR as u32
    }

    /// Check if opened with O_APPEND
    pub fn is_append(&self) -> bool {
        self.flags & libc::O_APPEND as u32 != 0
    }

    /// Check if opened with O_TRUNC
    pub fn is_truncate(&self) -> bool {
        self.flags & libc::O_TRUNC as u32 != 0
    }

    /// Update position after read/write
    pub fn update_position(&mut self, offset: u64, size: u64) {
        self.position = offset + size;
        self.last_access = Instant::now();
    }

    /// Mark as dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl Default for HandleManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HandleManager {
    /// Create a new handle manager
    pub fn new() -> Self {
        Self {
            handles: DashMap::new(),
            next_handle: AtomicU64::new(1),
        }
    }

    /// Open a new handle
    pub fn open(&self, inode: u64, flags: u32, is_dir: bool) -> u64 {
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        let state = HandleState::new(inode, flags, is_dir);

        debug!(
            handle = handle,
            inode = inode,
            flags = flags,
            writable = state.writable,
            readable = state.readable,
            "Opened handle"
        );

        self.handles.insert(handle, state);
        handle
    }

    /// Get handle state
    pub fn get(&self, handle: u64) -> Option<HandleState> {
        self.handles.get(&handle).map(|v| v.clone())
    }

    /// Update handle state
    pub fn update<F>(&self, handle: u64, f: F)
    where
        F: FnOnce(&mut HandleState),
    {
        if let Some(mut state) = self.handles.get_mut(&handle) {
            f(&mut state);
        }
    }

    /// Close a handle
    pub fn close(&self, handle: u64) -> Option<HandleState> {
        let result = self.handles.remove(&handle).map(|(_, v)| v);
        if result.is_some() {
            debug!(handle = handle, "Closed handle");
        }
        result
    }

    /// Get all handles for an inode
    pub fn handles_for_inode(&self, inode: u64) -> Vec<u64> {
        self.handles
            .iter()
            .filter(|entry| entry.value().inode == inode)
            .map(|entry| *entry.key())
            .collect()
    }

    /// Check if inode has any open handles
    pub fn has_open_handles(&self, inode: u64) -> bool {
        self.handles.iter().any(|entry| entry.value().inode == inode)
    }

    /// Check if inode has any writable handles
    pub fn has_writable_handles(&self, inode: u64) -> bool {
        self.handles
            .iter()
            .any(|entry| entry.value().inode == inode && entry.value().writable)
    }

    /// Get all dirty handles
    pub fn dirty_handles(&self) -> Vec<u64> {
        self.handles
            .iter()
            .filter(|entry| entry.value().dirty)
            .map(|entry| *entry.key())
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> HandleStats {
        let mut total = 0;
        let mut readable = 0;
        let mut writable = 0;
        let mut dirty = 0;
        let mut directories = 0;

        for entry in self.handles.iter() {
            total += 1;
            if entry.value().readable {
                readable += 1;
            }
            if entry.value().writable {
                writable += 1;
            }
            if entry.value().dirty {
                dirty += 1;
            }
            if entry.value().is_dir {
                directories += 1;
            }
        }

        HandleStats {
            total,
            readable,
            writable,
            dirty,
            directories,
        }
    }
}

/// Handle manager statistics
#[derive(Debug, Clone)]
pub struct HandleStats {
    pub total: usize,
    pub readable: usize,
    pub writable: usize,
    pub dirty: usize,
    pub directories: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_manager() {
        let manager = HandleManager::new();

        let h1 = manager.open(1, libc::O_RDONLY as u32, false);
        let h2 = manager.open(1, libc::O_RDWR as u32, false);
        let h3 = manager.open(2, libc::O_WRONLY as u32, false);

        assert!(manager.get(h1).is_some());
        assert!(manager.get(h2).is_some());
        assert!(manager.get(h3).is_some());

        let handles = manager.handles_for_inode(1);
        assert_eq!(handles.len(), 2);

        assert!(manager.has_writable_handles(1));
        assert!(manager.has_writable_handles(2));

        manager.close(h1);
        assert!(manager.get(h1).is_none());
    }

    #[test]
    fn test_handle_state_flags() {
        let state = HandleState::new(1, libc::O_RDONLY as u32, false);
        assert!(state.readable);
        assert!(!state.writable);

        let state = HandleState::new(1, libc::O_WRONLY as u32, false);
        assert!(!state.readable);
        assert!(state.writable);

        let state = HandleState::new(1, libc::O_RDWR as u32, false);
        assert!(state.readable);
        assert!(state.writable);

        let state = HandleState::new(1, (libc::O_WRONLY | libc::O_APPEND) as u32, false);
        assert!(state.is_append());
    }
}
