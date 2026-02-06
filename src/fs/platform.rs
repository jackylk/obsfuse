//! Platform abstraction layer for cross-platform filesystem support
//!
//! This module provides platform-agnostic types and functions that abstract
//! differences between Unix (FUSE) and Windows (WinFSP) implementations.

use std::time::SystemTime;

/// File type enumeration (platform-agnostic)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Regular file
    RegularFile,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Block device
    BlockDevice,
    /// Character device
    CharDevice,
    /// Named pipe (FIFO)
    NamedPipe,
    /// Unix domain socket
    Socket,
}

impl Default for FileKind {
    fn default() -> Self {
        FileKind::RegularFile
    }
}

#[cfg(unix)]
impl From<fuse3::FileType> for FileKind {
    fn from(ft: fuse3::FileType) -> Self {
        match ft {
            fuse3::FileType::Directory => FileKind::Directory,
            fuse3::FileType::RegularFile => FileKind::RegularFile,
            fuse3::FileType::Symlink => FileKind::Symlink,
            fuse3::FileType::BlockDevice => FileKind::BlockDevice,
            fuse3::FileType::CharDevice => FileKind::CharDevice,
            fuse3::FileType::NamedPipe => FileKind::NamedPipe,
            fuse3::FileType::Socket => FileKind::Socket,
        }
    }
}

#[cfg(unix)]
impl From<FileKind> for fuse3::FileType {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Directory => fuse3::FileType::Directory,
            FileKind::RegularFile => fuse3::FileType::RegularFile,
            FileKind::Symlink => fuse3::FileType::Symlink,
            FileKind::BlockDevice => fuse3::FileType::BlockDevice,
            FileKind::CharDevice => fuse3::FileType::CharDevice,
            FileKind::NamedPipe => fuse3::FileType::NamedPipe,
            FileKind::Socket => fuse3::FileType::Socket,
        }
    }
}

#[cfg(windows)]
impl FileKind {
    /// Convert to Windows file attributes
    pub fn to_win_attrs(&self) -> u32 {
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        match self {
            FileKind::Directory => FILE_ATTRIBUTE_DIRECTORY.0,
            FileKind::Symlink => FILE_ATTRIBUTE_REPARSE_POINT.0,
            _ => FILE_ATTRIBUTE_NORMAL.0,
        }
    }

    /// Create from Windows file attributes
    pub fn from_win_attrs(attrs: u32) -> Self {
        use windows::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        if attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            FileKind::Directory
        } else if attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            FileKind::Symlink
        } else {
            FileKind::RegularFile
        }
    }
}

/// Get current user ID
#[cfg(unix)]
pub fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(windows)]
pub fn current_uid() -> u32 {
    0 // Windows doesn't use Unix-style UIDs
}

/// Get current group ID
#[cfg(unix)]
pub fn current_gid() -> u32 {
    unsafe { libc::getgid() }
}

#[cfg(windows)]
pub fn current_gid() -> u32 {
    0 // Windows doesn't use Unix-style GIDs
}

/// Open file access mode flags
pub mod open_flags {
    /// Read-only access
    pub const O_RDONLY: u32 = 0;
    /// Write-only access
    pub const O_WRONLY: u32 = 1;
    /// Read-write access
    pub const O_RDWR: u32 = 2;
    /// Access mode mask
    pub const O_ACCMODE: u32 = 3;
    /// Append mode
    #[cfg(unix)]
    pub const O_APPEND: u32 = libc::O_APPEND as u32;
    #[cfg(windows)]
    pub const O_APPEND: u32 = 0x0008;
    /// Truncate on open
    #[cfg(unix)]
    pub const O_TRUNC: u32 = libc::O_TRUNC as u32;
    #[cfg(windows)]
    pub const O_TRUNC: u32 = 0x0200;
    /// Create if not exists
    #[cfg(unix)]
    pub const O_CREAT: u32 = libc::O_CREAT as u32;
    #[cfg(windows)]
    pub const O_CREAT: u32 = 0x0100;
    /// Exclusive create
    #[cfg(unix)]
    pub const O_EXCL: u32 = libc::O_EXCL as u32;
    #[cfg(windows)]
    pub const O_EXCL: u32 = 0x0400;
}

/// Check if flags indicate writable access
pub fn is_writable(flags: u32) -> bool {
    let access_mode = flags & open_flags::O_ACCMODE;
    access_mode == open_flags::O_WRONLY || access_mode == open_flags::O_RDWR
}

/// Check if flags indicate readable access
pub fn is_readable(flags: u32) -> bool {
    let access_mode = flags & open_flags::O_ACCMODE;
    access_mode == open_flags::O_RDONLY || access_mode == open_flags::O_RDWR
}

/// Check if append mode is set
pub fn is_append(flags: u32) -> bool {
    flags & open_flags::O_APPEND != 0
}

/// Check if truncate mode is set
pub fn is_truncate(flags: u32) -> bool {
    flags & open_flags::O_TRUNC != 0
}

/// Error codes (platform-agnostic)
pub mod errno {
    /// No such file or directory
    #[cfg(unix)]
    pub const ENOENT: i32 = libc::ENOENT;
    #[cfg(windows)]
    pub const ENOENT: i32 = 2;

    /// I/O error
    #[cfg(unix)]
    pub const EIO: i32 = libc::EIO;
    #[cfg(windows)]
    pub const EIO: i32 = 5;

    /// Directory not empty
    #[cfg(unix)]
    pub const ENOTEMPTY: i32 = libc::ENOTEMPTY;
    #[cfg(windows)]
    pub const ENOTEMPTY: i32 = 145;

    /// File exists
    #[cfg(unix)]
    pub const EEXIST: i32 = libc::EEXIST;
    #[cfg(windows)]
    pub const EEXIST: i32 = 17;

    /// Not a directory
    #[cfg(unix)]
    pub const ENOTDIR: i32 = libc::ENOTDIR;
    #[cfg(windows)]
    pub const ENOTDIR: i32 = 20;

    /// Is a directory
    #[cfg(unix)]
    pub const EISDIR: i32 = libc::EISDIR;
    #[cfg(windows)]
    pub const EISDIR: i32 = 21;

    /// Invalid argument
    #[cfg(unix)]
    pub const EINVAL: i32 = libc::EINVAL;
    #[cfg(windows)]
    pub const EINVAL: i32 = 22;

    /// Permission denied
    #[cfg(unix)]
    pub const EACCES: i32 = libc::EACCES;
    #[cfg(windows)]
    pub const EACCES: i32 = 13;

    /// No space left on device
    #[cfg(unix)]
    pub const ENOSPC: i32 = libc::ENOSPC;
    #[cfg(windows)]
    pub const ENOSPC: i32 = 28;

    /// Read-only file system
    #[cfg(unix)]
    pub const EROFS: i32 = libc::EROFS;
    #[cfg(windows)]
    pub const EROFS: i32 = 30;

    /// Function not implemented
    #[cfg(unix)]
    pub const ENOSYS: i32 = libc::ENOSYS;
    #[cfg(windows)]
    pub const ENOSYS: i32 = 38;
}

/// Platform-specific timestamp handling
#[derive(Debug, Clone, Copy)]
pub struct Timestamp {
    pub sec: i64,
    pub nsec: u32,
}

impl From<SystemTime> for Timestamp {
    fn from(st: SystemTime) -> Self {
        match st.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => Timestamp {
                sec: duration.as_secs() as i64,
                nsec: duration.subsec_nanos(),
            },
            Err(_) => Timestamp { sec: 0, nsec: 0 },
        }
    }
}

impl From<Timestamp> for SystemTime {
    fn from(ts: Timestamp) -> Self {
        let duration = std::time::Duration::new(ts.sec as u64, ts.nsec);
        if ts.sec >= 0 {
            std::time::UNIX_EPOCH + duration
        } else {
            std::time::UNIX_EPOCH
        }
    }
}

#[cfg(unix)]
impl From<fuse3::Timestamp> for Timestamp {
    fn from(ts: fuse3::Timestamp) -> Self {
        Timestamp {
            sec: ts.sec,
            nsec: ts.nsec,
        }
    }
}

#[cfg(unix)]
impl From<Timestamp> for fuse3::Timestamp {
    fn from(ts: Timestamp) -> Self {
        fuse3::Timestamp {
            sec: ts.sec,
            nsec: ts.nsec,
        }
    }
}

#[cfg(windows)]
impl Timestamp {
    /// Convert to Windows FILETIME (100-nanosecond intervals since Jan 1, 1601)
    pub fn to_filetime(&self) -> u64 {
        // Offset between Unix epoch (1970) and Windows epoch (1601) in 100-ns intervals
        const EPOCH_DIFF: u64 = 116444736000000000;
        let unix_100ns = (self.sec as u64) * 10_000_000 + (self.nsec as u64) / 100;
        unix_100ns + EPOCH_DIFF
    }

    /// Create from Windows FILETIME
    pub fn from_filetime(ft: u64) -> Self {
        const EPOCH_DIFF: u64 = 116444736000000000;
        if ft < EPOCH_DIFF {
            return Timestamp { sec: 0, nsec: 0 };
        }
        let unix_100ns = ft - EPOCH_DIFF;
        Timestamp {
            sec: (unix_100ns / 10_000_000) as i64,
            nsec: ((unix_100ns % 10_000_000) * 100) as u32,
        }
    }
}

/// Normalize path separators to Unix style
pub fn normalize_path(path: &str) -> String {
    #[cfg(windows)]
    {
        path.replace('\\', "/").trim_matches('/').to_string()
    }
    #[cfg(unix)]
    {
        path.trim_matches('/').to_string()
    }
}

/// Convert path to platform-native format
#[cfg(windows)]
pub fn to_native_path(path: &str) -> String {
    path.replace('/', "\\")
}

#[cfg(unix)]
pub fn to_native_path(path: &str) -> String {
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_kind_default() {
        assert_eq!(FileKind::default(), FileKind::RegularFile);
    }

    #[test]
    fn test_open_flags() {
        assert!(is_readable(open_flags::O_RDONLY));
        assert!(!is_writable(open_flags::O_RDONLY));

        assert!(!is_readable(open_flags::O_WRONLY));
        assert!(is_writable(open_flags::O_WRONLY));

        assert!(is_readable(open_flags::O_RDWR));
        assert!(is_writable(open_flags::O_RDWR));
    }

    #[test]
    fn test_timestamp_conversion() {
        let now = SystemTime::now();
        let ts: Timestamp = now.into();
        let back: SystemTime = ts.into();

        // Should be within 1 second due to conversion precision
        let diff = now.duration_since(back).unwrap_or_default();
        assert!(diff.as_secs() < 1);
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/foo/bar/"), "foo/bar");
        assert_eq!(normalize_path("foo/bar"), "foo/bar");
        assert_eq!(normalize_path("/"), "");
    }
}
