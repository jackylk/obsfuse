//! Directory operations for OBS FUSE filesystem
//!
//! This module handles directory listing and entry management.

use fuse3::FileType;
use std::ffi::OsString;

/// Directory entry for readdir operations
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// Inode number
    pub inode: u64,
    /// Entry name
    pub name: OsString,
    /// File type
    pub kind: FileType,
    /// Offset for next entry
    pub offset: i64,
}

impl DirEntry {
    /// Create a new directory entry
    pub fn new(inode: u64, name: impl Into<OsString>, kind: FileType, offset: i64) -> Self {
        Self {
            inode,
            name: name.into(),
            kind,
            offset,
        }
    }

    /// Create entry for "."
    pub fn dot(inode: u64) -> Self {
        Self::new(inode, ".", FileType::Directory, 1)
    }

    /// Create entry for ".."
    pub fn dotdot(parent_inode: u64) -> Self {
        Self::new(parent_inode, "..", FileType::Directory, 2)
    }
}

/// Directory entry builder for readdir plus operations
#[derive(Debug, Clone)]
pub struct DirEntryPlus {
    /// Basic entry info
    pub entry: DirEntry,
    /// Full file attributes
    pub attr: Option<crate::fs::inode::FileAttr>,
    /// Attribute TTL
    pub attr_ttl: std::time::Duration,
    /// Entry TTL
    pub entry_ttl: std::time::Duration,
}

impl DirEntryPlus {
    /// Create a new directory entry plus
    pub fn new(entry: DirEntry, attr: Option<crate::fs::inode::FileAttr>) -> Self {
        Self {
            entry,
            attr,
            attr_ttl: std::time::Duration::from_secs(1),
            entry_ttl: std::time::Duration::from_secs(1),
        }
    }

    /// Set TTLs
    pub fn with_ttl(mut self, attr_ttl: std::time::Duration, entry_ttl: std::time::Duration) -> Self {
        self.attr_ttl = attr_ttl;
        self.entry_ttl = entry_ttl;
        self
    }
}

/// Directory listing result
#[derive(Debug, Clone)]
pub struct DirListing {
    /// Directory entries
    pub entries: Vec<DirEntry>,
    /// Whether listing is complete
    pub complete: bool,
    /// Continuation token for pagination
    pub continuation_token: Option<String>,
}

impl DirListing {
    /// Create a new directory listing
    pub fn new(entries: Vec<DirEntry>) -> Self {
        Self {
            entries,
            complete: true,
            continuation_token: None,
        }
    }

    /// Create with pagination info
    pub fn with_pagination(
        entries: Vec<DirEntry>,
        complete: bool,
        continuation_token: Option<String>,
    ) -> Self {
        Self {
            entries,
            complete,
            continuation_token,
        }
    }

    /// Get entries starting from offset
    pub fn from_offset(&self, offset: i64) -> impl Iterator<Item = &DirEntry> {
        self.entries
            .iter()
            .filter(move |e| e.offset > offset)
    }
}

/// Extract directory name from OBS path
pub fn extract_dir_name(path: &str, prefix: &str) -> Option<String> {
    let relative = path.strip_prefix(prefix)?.trim_start_matches('/');

    // Get first component
    let name = relative.split('/').next()?;

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Check if path is direct child of prefix
pub fn is_direct_child(path: &str, prefix: &str) -> bool {
    let relative = match path.strip_prefix(prefix) {
        Some(r) => r.trim_start_matches('/'),
        None => return false,
    };

    // Direct child has no '/' or only trailing '/'
    let slash_count = relative.trim_end_matches('/').matches('/').count();
    slash_count == 0 && !relative.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_entry() {
        let entry = DirEntry::new(42, "test.txt", FileType::RegularFile, 1);
        assert_eq!(entry.inode, 42);
        assert_eq!(entry.name, OsString::from("test.txt"));
        assert_eq!(entry.kind, FileType::RegularFile);
    }

    #[test]
    fn test_dot_entries() {
        let dot = DirEntry::dot(1);
        assert_eq!(dot.name, OsString::from("."));
        assert_eq!(dot.offset, 1);

        let dotdot = DirEntry::dotdot(2);
        assert_eq!(dotdot.name, OsString::from(".."));
        assert_eq!(dotdot.offset, 2);
    }

    #[test]
    fn test_extract_dir_name() {
        assert_eq!(
            extract_dir_name("prefix/dir/file.txt", "prefix/"),
            Some("dir".to_string())
        );
        assert_eq!(
            extract_dir_name("prefix/file.txt", "prefix/"),
            Some("file.txt".to_string())
        );
        assert_eq!(extract_dir_name("other/file.txt", "prefix/"), None);
    }

    #[test]
    fn test_is_direct_child() {
        assert!(is_direct_child("prefix/file.txt", "prefix/"));
        assert!(is_direct_child("prefix/dir/", "prefix/"));
        assert!(!is_direct_child("prefix/dir/file.txt", "prefix/"));
        assert!(!is_direct_child("other/file.txt", "prefix/"));
    }
}
