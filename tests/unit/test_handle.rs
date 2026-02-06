//! Comprehensive unit tests for Handle Manager
//!
//! Tests cover:
//! - Handle creation and lookup
//! - Open flags parsing
//! - Handle state tracking
//! - Reference management

#[cfg(test)]
mod tests {
    use obsfuse::fs::handle::{HandleManager, HandleState, HandleStats};

    // ==================== Handle Creation ====================

    #[test]
    fn test_handle_creation() {
        let manager = HandleManager::new();

        let h1 = manager.open(1, libc::O_RDONLY as u32, false);
        let h2 = manager.open(2, libc::O_RDONLY as u32, false);

        assert_ne!(h1, h2);
        assert!(h1 > 0);
        assert!(h2 > 0);
    }

    #[test]
    fn test_handle_get() {
        let manager = HandleManager::new();

        let h = manager.open(42, libc::O_RDONLY as u32, false);

        let state = manager.get(h);
        assert!(state.is_some());

        let state = state.unwrap();
        assert_eq!(state.inode, 42);
    }

    #[test]
    fn test_handle_get_non_existing() {
        let manager = HandleManager::new();
        assert!(manager.get(99999).is_none());
    }

    #[test]
    fn test_handle_close() {
        let manager = HandleManager::new();

        let h = manager.open(1, libc::O_RDONLY as u32, false);
        assert!(manager.get(h).is_some());

        let closed = manager.close(h);
        assert!(closed.is_some());
        assert!(manager.get(h).is_none());
    }

    #[test]
    fn test_handle_close_non_existing() {
        let manager = HandleManager::new();
        assert!(manager.close(99999).is_none());
    }

    // ==================== Open Flags ====================

    #[test]
    fn test_rdonly_flags() {
        let state = HandleState::new(1, libc::O_RDONLY as u32, false);

        assert!(state.readable);
        assert!(!state.writable);
    }

    #[test]
    fn test_wronly_flags() {
        let state = HandleState::new(1, libc::O_WRONLY as u32, false);

        assert!(!state.readable);
        assert!(state.writable);
    }

    #[test]
    fn test_rdwr_flags() {
        let state = HandleState::new(1, libc::O_RDWR as u32, false);

        assert!(state.readable);
        assert!(state.writable);
    }

    #[test]
    fn test_append_flag() {
        let flags = (libc::O_WRONLY | libc::O_APPEND) as u32;
        let state = HandleState::new(1, flags, false);

        assert!(state.is_append());
    }

    #[test]
    fn test_truncate_flag() {
        let flags = (libc::O_WRONLY | libc::O_TRUNC) as u32;
        let state = HandleState::new(1, flags, false);

        assert!(state.is_truncate());
    }

    #[test]
    fn test_directory_handle() {
        let manager = HandleManager::new();

        let h = manager.open(1, libc::O_RDONLY as u32, true);
        let state = manager.get(h).unwrap();

        assert!(state.is_dir);
    }

    // ==================== Handle State Updates ====================

    #[test]
    fn test_update_position() {
        let manager = HandleManager::new();

        let h = manager.open(1, libc::O_RDONLY as u32, false);

        manager.update(h, |state| {
            state.update_position(0, 1024);
        });

        let state = manager.get(h).unwrap();
        assert_eq!(state.position, 1024);

        manager.update(h, |state| {
            state.update_position(1024, 512);
        });

        let state = manager.get(h).unwrap();
        assert_eq!(state.position, 1536);
    }

    #[test]
    fn test_mark_dirty() {
        let manager = HandleManager::new();

        let h = manager.open(1, libc::O_WRONLY as u32, false);

        let state = manager.get(h).unwrap();
        assert!(!state.dirty);

        manager.update(h, |state| {
            state.mark_dirty();
        });

        let state = manager.get(h).unwrap();
        assert!(state.dirty);
    }

    // ==================== Handle Queries ====================

    #[test]
    fn test_handles_for_inode() {
        let manager = HandleManager::new();

        let h1 = manager.open(1, libc::O_RDONLY as u32, false);
        let h2 = manager.open(1, libc::O_RDWR as u32, false);
        let h3 = manager.open(2, libc::O_RDONLY as u32, false);

        let handles = manager.handles_for_inode(1);
        assert_eq!(handles.len(), 2);
        assert!(handles.contains(&h1));
        assert!(handles.contains(&h2));
        assert!(!handles.contains(&h3));
    }

    #[test]
    fn test_has_open_handles() {
        let manager = HandleManager::new();

        assert!(!manager.has_open_handles(1));

        let h = manager.open(1, libc::O_RDONLY as u32, false);
        assert!(manager.has_open_handles(1));

        manager.close(h);
        assert!(!manager.has_open_handles(1));
    }

    #[test]
    fn test_has_writable_handles() {
        let manager = HandleManager::new();

        let h1 = manager.open(1, libc::O_RDONLY as u32, false);
        assert!(!manager.has_writable_handles(1));

        let h2 = manager.open(1, libc::O_WRONLY as u32, false);
        assert!(manager.has_writable_handles(1));

        manager.close(h2);
        assert!(!manager.has_writable_handles(1));
    }

    #[test]
    fn test_dirty_handles() {
        let manager = HandleManager::new();

        let h1 = manager.open(1, libc::O_WRONLY as u32, false);
        let h2 = manager.open(2, libc::O_WRONLY as u32, false);

        manager.update(h1, |state| {
            state.mark_dirty();
        });

        let dirty = manager.dirty_handles();
        assert_eq!(dirty.len(), 1);
        assert!(dirty.contains(&h1));
    }

    // ==================== Statistics ====================

    #[test]
    fn test_handle_stats() {
        let manager = HandleManager::new();

        // Empty
        let stats = manager.stats();
        assert_eq!(stats.total, 0);

        // Add various handles
        manager.open(1, libc::O_RDONLY as u32, false); // readable
        manager.open(2, libc::O_WRONLY as u32, false); // writable
        manager.open(3, libc::O_RDWR as u32, false); // both
        manager.open(4, libc::O_RDONLY as u32, true); // directory

        let stats = manager.stats();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.readable, 3); // RDONLY + RDWR + dir
        assert_eq!(stats.writable, 2); // WRONLY + RDWR
        assert_eq!(stats.directories, 1);
        assert_eq!(stats.dirty, 0);
    }

    // ==================== Concurrent Access ====================

    #[test]
    fn test_concurrent_handle_creation() {
        use std::sync::Arc;
        use std::thread;

        let manager = Arc::new(HandleManager::new());
        let mut handles_results = vec![];

        for _ in 0..10 {
            let manager = Arc::clone(&manager);
            handles_results.push(thread::spawn(move || {
                manager.open(1, libc::O_RDONLY as u32, false)
            }));
        }

        let handles: Vec<u64> = handles_results
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        // All handles should be unique
        let mut sorted = handles.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }
}
