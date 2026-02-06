//! Cache layer for OBS FUSE filesystem
//!
//! This module provides multi-level caching for metadata and data
//! to optimize performance and reduce API calls.

pub mod data;
pub mod metadata;
pub mod readahead;
pub mod write_buffer;

pub use data::{BlockKey, DataCache, DataCacheStats};
pub use metadata::{CachedAttr, CachedDirEntry, MetadataCache, MetadataCacheStats};
pub use readahead::{prefetch_task, PrefetchRequest, ReadaheadConfig, ReadaheadManager, ReadaheadStats};
pub use write_buffer::{flush_task, FileWriteBuffer, WriteBuffer, WriteBufferConfig, WriteBufferStats};
