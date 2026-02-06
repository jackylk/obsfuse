//! Comprehensive unit tests for Directory operations
//!
//! Tests cover:
//! - Directory entry creation
//! - Directory listing
//! - Path utilities

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::time::Duration;

    use fuse3::FileType;

    use obsfuse::fs::dir::{DirEntry, DirEntryPlus, DirListing, extract_dir_name, is_direct_child};
    use obsfuse::fs::inode::FileAttr;

    // ==================== DirEntry Tests ====================

    #[test]
    fn test_dir_entry_creation() {
        let entry = DirEntry::new(42, "test.txt", FileType::RegularFile, 1);

        assert_eq!(entry.inode, 42);
        assert_eq!(entry.name, OsString::from("test.txt"));
        assert_eq!(entry.kind, FileType::RegularFile);
        assert_eq!(entry.offset, 1);
    }

    #[test]
    fn test_dir_entry_dot() {
        let entry = DirEntry::dot(42);

        assert_eq!(entry.inode, 42);
        assert_eq!(entry.name, OsString::from("."));
        assert_eq!(entry.kind, FileType::Directory);
        assert_eq!(entry.offset, 1);
    }

    #[test]
    fn test_dir_entry_dotdot() {
        let entry = DirEntry::dotdot(1);

        assert_eq!(entry.inode, 1);
        assert_eq!(entry.name, OsString::from(".."));
        assert_eq!(entry.kind, FileType::Directory);
        assert_eq!(entry.offset, 2);
    }

    #[test]
    fn test_dir_entry_directory() {
        let entry = DirEntry::new(10, "subdir", FileType::Directory, 5);

        assert_eq!(entry.kind, FileType::Directory);
    }

    // ==================== DirEntryPlus Tests ====================

    #[test]
    fn test_dir_entry_plus_creation() {
        let entry = DirEntry::new(42, "file.txt", FileType::RegularFile, 1);
        let attr = FileAttr::file(42, 1024, 1000, 1000, 0o644);

        let plus = DirEntryPlus::new(entry.clone(), Some(attr.clone()));

        assert_eq!(plus.entry.inode, 42);
        assert!(plus.attr.is_some());
        assert_eq!(plus.attr.unwrap().size, 1024);
    }

    #[test]
    fn test_dir_entry_plus_no_attr() {
        let entry = DirEntry::new(42, "file.txt", FileType::RegularFile, 1);
        let plus = DirEntryPlus::new(entry, None);

        assert!(plus.attr.is_none());
    }

    #[test]
    fn test_dir_entry_plus_with_ttl() {
        let entry = DirEntry::new(42, "file.txt", FileType::RegularFile, 1);
        let plus = DirEntryPlus::new(entry, None)
            .with_ttl(Duration::from_secs(10), Duration::from_secs(5));

        assert_eq!(plus.attr_ttl, Duration::from_secs(10));
        assert_eq!(plus.entry_ttl, Duration::from_secs(5));
    }

    // ==================== DirListing Tests ====================

    #[test]
    fn test_dir_listing_new() {
        let entries = vec![
            DirEntry::dot(1),
            DirEntry::dotdot(1),
            DirEntry::new(2, "file.txt", FileType::RegularFile, 3),
        ];

        let listing = DirListing::new(entries);

        assert_eq!(listing.entries.len(), 3);
        assert!(listing.complete);
        assert!(listing.continuation_token.is_none());
    }

    #[test]
    fn test_dir_listing_with_pagination() {
        let entries = vec![
            DirEntry::new(2, "file1.txt", FileType::RegularFile, 1),
            DirEntry::new(3, "file2.txt", FileType::RegularFile, 2),
        ];

        let listing = DirListing::with_pagination(
            entries,
            false,
            Some("next-token".to_string()),
        );

        assert!(!listing.complete);
        assert_eq!(listing.continuation_token, Some("next-token".to_string()));
    }

    #[test]
    fn test_dir_listing_from_offset() {
        let entries = vec![
            DirEntry::new(1, "a", FileType::RegularFile, 1),
            DirEntry::new(2, "b", FileType::RegularFile, 2),
            DirEntry::new(3, "c", FileType::RegularFile, 3),
            DirEntry::new(4, "d", FileType::RegularFile, 4),
        ];

        let listing = DirListing::new(entries);

        // Get entries after offset 2
        let after_2: Vec<_> = listing.from_offset(2).collect();
        assert_eq!(after_2.len(), 2);
        assert_eq!(after_2[0].name, OsString::from("c"));
        assert_eq!(after_2[1].name, OsString::from("d"));
    }

    #[test]
    fn test_dir_listing_from_offset_zero() {
        let entries = vec![
            DirEntry::new(1, "a", FileType::RegularFile, 1),
            DirEntry::new(2, "b", FileType::RegularFile, 2),
        ];

        let listing = DirListing::new(entries);

        let all: Vec<_> = listing.from_offset(0).collect();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_dir_listing_from_offset_past_end() {
        let entries = vec![
            DirEntry::new(1, "a", FileType::RegularFile, 1),
        ];

        let listing = DirListing::new(entries);

        let none: Vec<_> = listing.from_offset(10).collect();
        assert!(none.is_empty());
    }

    // ==================== Path Utilities ====================

    #[test]
    fn test_extract_dir_name_file() {
        let name = extract_dir_name("prefix/file.txt", "prefix/");
        assert_eq!(name, Some("file.txt".to_string()));
    }

    #[test]
    fn test_extract_dir_name_nested() {
        let name = extract_dir_name("prefix/dir/file.txt", "prefix/");
        assert_eq!(name, Some("dir".to_string()));
    }

    #[test]
    fn test_extract_dir_name_directory() {
        let name = extract_dir_name("prefix/subdir/", "prefix/");
        assert_eq!(name, Some("subdir".to_string()));
    }

    #[test]
    fn test_extract_dir_name_no_match() {
        let name = extract_dir_name("other/file.txt", "prefix/");
        assert!(name.is_none());
    }

    #[test]
    fn test_extract_dir_name_empty_prefix() {
        let name = extract_dir_name("file.txt", "");
        assert_eq!(name, Some("file.txt".to_string()));
    }

    #[test]
    fn test_is_direct_child_file() {
        assert!(is_direct_child("prefix/file.txt", "prefix/"));
    }

    #[test]
    fn test_is_direct_child_directory() {
        assert!(is_direct_child("prefix/subdir/", "prefix/"));
    }

    #[test]
    fn test_is_direct_child_nested() {
        // Nested files are NOT direct children
        assert!(!is_direct_child("prefix/dir/file.txt", "prefix/"));
    }

    #[test]
    fn test_is_direct_child_no_match() {
        assert!(!is_direct_child("other/file.txt", "prefix/"));
    }

    #[test]
    fn test_is_direct_child_empty() {
        assert!(!is_direct_child("prefix/", "prefix/"));
    }
}
