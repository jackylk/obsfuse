//! Local component benchmarks (no network dependency)

use bytes::Bytes;
use bytesize::ByteSize;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;

use obsfuse::cache::{DataCache, MetadataCache, ReadaheadConfig, ReadaheadManager};
use obsfuse::config::{CacheConfig, FixedPermission, MetadataCacheConfig, PermissionConfig, PermissionMode};
use obsfuse::fs::inode::FileAttr;
use obsfuse::fs::InodeManager;
use obsfuse::utils::Metrics;

fn create_inode_manager() -> InodeManager {
    let config = PermissionConfig {
        mode: PermissionMode::Fixed,
        fixed: FixedPermission {
            uid: 1000,
            gid: 1000,
            file_mode: 0o644,
            dir_mode: 0o755,
        },
    };
    InodeManager::new(config)
}

fn create_metadata_cache() -> MetadataCache {
    let config = MetadataCacheConfig {
        attr_ttl: Duration::from_secs(60),
        dir_ttl: Duration::from_secs(60),
        negative_ttl: Duration::from_secs(60),
        max_entries: 100000,
    };
    MetadataCache::new(config, Arc::new(Metrics::new()))
}

fn create_data_cache() -> DataCache {
    let config = CacheConfig {
        memory_limit: ByteSize::mb(256),
        disk_limit: ByteSize::mb(0),
        cache_dir: None,
        block_size: ByteSize::kb(64),
        metadata: Default::default(),
    };
    DataCache::new(&config, Arc::new(Metrics::new()))
}

fn create_readahead_manager() -> ReadaheadManager {
    let config = ReadaheadConfig {
        enable: true,
        window_size: 16 * 1024 * 1024,
        concurrency: 4,
        seq_threshold: 3,
        prefetch_ttl: Duration::from_secs(30),
    };
    ReadaheadManager::new(config, Arc::new(Metrics::new()))
}

// ============================================================================
// Inode Benchmarks
// ============================================================================

pub fn inode_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("inode");

    // Benchmark: Create new inode
    group.bench_function("create", |b| {
        let manager = create_inode_manager();
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let path = format!("bench/path/{}/file.txt", counter);
            black_box(manager.get_or_create_inode(&path, false, 1024))
        });
    });

    // Benchmark: Lookup existing inode
    group.bench_function("lookup_existing", |b| {
        let manager = create_inode_manager();
        // Pre-create inodes
        for i in 0..10000 {
            manager.get_or_create_inode(&format!("existing/{}", i), false, 100);
        }

        let mut counter = 0u64;
        b.iter(|| {
            counter = (counter + 1) % 10000;
            black_box(manager.get_inode(&format!("existing/{}", counter)))
        });
    });

    // Benchmark: Get path from inode
    group.bench_function("get_path", |b| {
        let manager = create_inode_manager();
        let inode = manager.get_or_create_inode("test/benchmark/path/file.txt", false, 100);

        b.iter(|| {
            black_box(manager.get_path(inode))
        });
    });

    // Benchmark: Update size
    group.bench_function("update_size", |b| {
        let manager = create_inode_manager();
        let inode = manager.get_or_create_inode("test/file.txt", false, 100);

        let mut size = 0u64;
        b.iter(|| {
            size += 1024;
            manager.update_size(inode, size);
        });
    });

    // Benchmark: Rename operation
    group.bench_function("rename", |b| {
        let manager = create_inode_manager();
        let mut counter = 0u64;

        b.iter(|| {
            counter += 1;
            let old_path = format!("rename/old/{}", counter);
            let new_path = format!("rename/new/{}", counter);
            manager.get_or_create_inode(&old_path, false, 100);
            manager.rename(&old_path, &new_path);
        });
    });

    group.finish();
}

// ============================================================================
// Cache Benchmarks
// ============================================================================

pub fn cache_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");

    // Metadata cache benchmarks
    group.bench_function("metadata_put", |b| {
        let cache = create_metadata_cache();
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            cache.put_attr(counter, FileAttr::default());
        });
    });

    group.bench_function("metadata_get_hit", |b| {
        let cache = create_metadata_cache();
        // Pre-populate
        for i in 0..10000 {
            cache.put_attr(i, FileAttr::default());
        }

        let mut counter = 0u64;
        b.iter(|| {
            counter = (counter + 1) % 10000;
            black_box(cache.get_attr(counter))
        });
    });

    group.bench_function("metadata_get_miss", |b| {
        let cache = create_metadata_cache();
        let mut counter = 100000u64;
        b.iter(|| {
            counter += 1;
            black_box(cache.get_attr(counter))
        });
    });

    // Data cache benchmarks with throughput
    for size in [4 * 1024, 64 * 1024, 256 * 1024].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(
            BenchmarkId::new("data_put", format!("{}KB", size / 1024)),
            size,
            |b, &size| {
                let cache = create_data_cache();
                let data = Bytes::from(vec![0u8; size]);
                let mut counter = 0u64;
                b.iter(|| {
                    counter += 1;
                    cache.put(1, counter * size as u64, data.clone());
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("data_get_hit", format!("{}KB", size / 1024)),
            size,
            |b, &size| {
                let cache = create_data_cache();
                let data = Bytes::from(vec![0u8; size]);
                // Pre-populate
                for i in 0..1000 {
                    cache.put(1, i * size as u64, data.clone());
                }

                let mut counter = 0u64;
                b.iter(|| {
                    counter = (counter + 1) % 1000;
                    black_box(cache.get(1, counter * size as u64))
                });
            },
        );
    }

    // Cache invalidation
    group.bench_function("data_invalidate", |b| {
        let cache = create_data_cache();
        let data = Bytes::from(vec![0u8; 65536]);

        b.iter_batched(
            || {
                // Setup: populate cache
                for i in 0..100 {
                    cache.put(1, i * 65536, data.clone());
                }
            },
            |_| {
                cache.invalidate(1);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ============================================================================
// Path Operation Benchmarks
// ============================================================================

pub fn path_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("path");

    group.bench_function("join", |b| {
        b.iter(|| {
            black_box(InodeManager::join_path("parent/dir/subdir", "child.txt"))
        });
    });

    group.bench_function("parent", |b| {
        b.iter(|| {
            black_box(InodeManager::parent_path("a/b/c/d/e/f/g/file.txt"))
        });
    });

    group.bench_function("file_name", |b| {
        b.iter(|| {
            black_box(InodeManager::file_name("a/b/c/d/e/f/g/file.txt"))
        });
    });

    // Deep path operations
    let deep_path = (0..20).map(|i| format!("dir{}", i)).collect::<Vec<_>>().join("/") + "/file.txt";

    group.bench_function("join_deep", |b| {
        b.iter(|| {
            black_box(InodeManager::join_path(&deep_path, "child.txt"))
        });
    });

    group.bench_function("parent_deep", |b| {
        b.iter(|| {
            black_box(InodeManager::parent_path(&deep_path))
        });
    });

    group.finish();
}

// ============================================================================
// Readahead Benchmarks
// ============================================================================

pub fn readahead_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("readahead");

    group.bench_function("record_read", |b| {
        let manager = create_readahead_manager();
        let mut offset = 0u64;
        b.iter(|| {
            offset += 4096;
            black_box(manager.record_read(1, offset, 4096))
        });
    });

    group.bench_function("sequential_detection", |b| {
        let manager = create_readahead_manager();
        b.iter_batched(
            || 0u64,
            |_| {
                // Simulate sequential reads
                for i in 0..10 {
                    black_box(manager.record_read(1, i * 4096, 4096));
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.throughput(Throughput::Bytes(65536));
    group.bench_function("store_prefetch", |b| {
        let manager = create_readahead_manager();
        let data = Bytes::from(vec![0u8; 65536]);
        let mut offset = 0u64;
        b.iter(|| {
            offset += 65536;
            manager.store_prefetched(1, offset, data.clone());
        });
    });

    group.bench_function("get_prefetch_hit", |b| {
        let manager = create_readahead_manager();
        let data = Bytes::from(vec![0u8; 65536]);
        // Pre-populate
        for i in 0..1000 {
            manager.store_prefetched(1, i * 65536, data.clone());
        }

        let mut counter = 0u64;
        b.iter(|| {
            counter = (counter + 1) % 1000;
            black_box(manager.get_prefetched(1, counter * 65536))
        });
    });

    group.finish();
}
