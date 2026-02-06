//! Benchmark tests for OBS FUSE
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::sync::Arc;

// Note: These benchmarks test internal components.
// Full filesystem benchmarks should use fio or similar tools.

fn benchmark_inode_operations(c: &mut Criterion) {
    use obsfuse::config::{FixedPermission, PermissionConfig, PermissionMode};
    use obsfuse::fs::InodeManager;

    let config = PermissionConfig {
        mode: PermissionMode::Fixed,
        fixed: FixedPermission {
            uid: 1000,
            gid: 1000,
            file_mode: 0o644,
            dir_mode: 0o755,
        },
    };

    let manager = InodeManager::new(config);

    let mut group = c.benchmark_group("inode_operations");

    group.bench_function("get_or_create_inode", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let path = format!("test/path/{}/file.txt", counter);
            black_box(manager.get_or_create_inode(&path, false, 1024));
        });
    });

    group.bench_function("get_inode_existing", |b| {
        // Pre-create some inodes
        for i in 0..1000 {
            manager.get_or_create_inode(&format!("existing/{}", i), false, 100);
        }

        b.iter(|| {
            black_box(manager.get_inode("existing/500"));
        });
    });

    group.bench_function("get_path", |b| {
        let inode = manager.get_or_create_inode("test/benchmark/path", false, 100);

        b.iter(|| {
            black_box(manager.get_path(inode));
        });
    });

    group.finish();
}

fn benchmark_cache_operations(c: &mut Criterion) {
    use obsfuse::cache::{DataCache, MetadataCache};
    use obsfuse::config::{CacheConfig, MetadataCacheConfig};
    use obsfuse::fs::inode::FileAttr;
    use obsfuse::utils::Metrics;
    use bytes::Bytes;
    use bytesize::ByteSize;
    use std::time::Duration;

    let metrics = Arc::new(Metrics::new());

    let mut group = c.benchmark_group("cache_operations");

    // Metadata cache benchmarks
    let metadata_config = MetadataCacheConfig {
        attr_ttl: Duration::from_secs(60),
        dir_ttl: Duration::from_secs(60),
        negative_ttl: Duration::from_secs(60),
        max_entries: 100000,
    };
    let metadata_cache = MetadataCache::new(metadata_config, metrics.clone());

    group.bench_function("metadata_put_attr", |b| {
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let attr = FileAttr::default();
            metadata_cache.put_attr(counter, attr);
        });
    });

    group.bench_function("metadata_get_attr", |b| {
        // Pre-populate
        for i in 0..10000 {
            metadata_cache.put_attr(i, FileAttr::default());
        }

        let mut counter = 0u64;
        b.iter(|| {
            counter = (counter + 1) % 10000;
            black_box(metadata_cache.get_attr(counter));
        });
    });

    // Data cache benchmarks
    let cache_config = CacheConfig {
        memory_limit: ByteSize::mb(100),
        disk_limit: ByteSize::mb(0),
        cache_dir: None,
        block_size: ByteSize::kb(64),
        metadata: Default::default(),
    };
    let data_cache = DataCache::new(&cache_config, metrics.clone());

    group.bench_function("data_cache_put", |b| {
        let data = Bytes::from(vec![0u8; 65536]);
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            data_cache.put(1, counter * 65536, data.clone());
        });
    });

    group.bench_function("data_cache_get", |b| {
        // Pre-populate
        let data = Bytes::from(vec![0u8; 65536]);
        for i in 0..1000 {
            data_cache.put(1, i * 65536, data.clone());
        }

        let mut counter = 0u64;
        b.iter(|| {
            counter = (counter + 1) % 1000;
            black_box(data_cache.get(1, counter * 65536));
        });
    });

    group.finish();
}

fn benchmark_path_operations(c: &mut Criterion) {
    use obsfuse::fs::InodeManager;

    let mut group = c.benchmark_group("path_operations");

    group.bench_function("join_path", |b| {
        b.iter(|| {
            black_box(InodeManager::join_path("parent/dir", "child.txt"));
        });
    });

    group.bench_function("parent_path", |b| {
        b.iter(|| {
            black_box(InodeManager::parent_path("a/b/c/d/e/f.txt"));
        });
    });

    group.bench_function("file_name", |b| {
        b.iter(|| {
            black_box(InodeManager::file_name("a/b/c/d/e/file.txt"));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_inode_operations,
    benchmark_cache_operations,
    benchmark_path_operations,
);

criterion_main!(benches);
