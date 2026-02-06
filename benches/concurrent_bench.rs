//! Concurrent benchmarks for multi-threaded performance testing

use bytes::Bytes;
use bytesize::ByteSize;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use obsfuse::cache::{DataCache, MetadataCache};
use obsfuse::config::{CacheConfig, FixedPermission, MetadataCacheConfig, PermissionConfig, PermissionMode};
use obsfuse::fs::inode::FileAttr;
use obsfuse::fs::InodeManager;
use obsfuse::utils::Metrics;

const THREAD_COUNTS: &[usize] = &[1, 2, 4, 8, 16, 32];

fn create_inode_manager() -> Arc<InodeManager> {
    let config = PermissionConfig {
        mode: PermissionMode::Fixed,
        fixed: FixedPermission {
            uid: 1000,
            gid: 1000,
            file_mode: 0o644,
            dir_mode: 0o755,
        },
    };
    Arc::new(InodeManager::new(config))
}

fn create_metadata_cache() -> Arc<MetadataCache> {
    let config = MetadataCacheConfig {
        attr_ttl: Duration::from_secs(60),
        dir_ttl: Duration::from_secs(60),
        negative_ttl: Duration::from_secs(60),
        max_entries: 1000000,
    };
    Arc::new(MetadataCache::new(config, Arc::new(Metrics::new())))
}

fn create_data_cache() -> Arc<DataCache> {
    let config = CacheConfig {
        memory_limit: ByteSize::mb(512),
        disk_limit: ByteSize::mb(0),
        cache_dir: None,
        block_size: ByteSize::kb(64),
        metadata: Default::default(),
    };
    Arc::new(DataCache::new(&config, Arc::new(Metrics::new())))
}

// ============================================================================
// Concurrent Inode Benchmarks
// ============================================================================

pub fn inode_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_inode");

    // Concurrent create (different paths)
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("create", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let manager = create_inode_manager();
                    let counter = Arc::new(AtomicU64::new(0));
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let manager = Arc::clone(&manager);
                            let counter = Arc::clone(&counter);
                            thread::spawn(move || {
                                for _ in 0..ops_per_thread {
                                    let id = counter.fetch_add(1, Ordering::Relaxed);
                                    let path = format!("thread{}/file{}.txt", t, id);
                                    black_box(manager.get_or_create_inode(&path, false, 1024));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    // Concurrent lookup (same paths, read contention)
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("lookup_contention", num_threads),
            &num_threads,
            |b, &num_threads| {
                let manager = create_inode_manager();
                // Pre-create shared data
                for i in 0..1000 {
                    manager.get_or_create_inode(&format!("shared/{}", i), false, 100);
                }
                let manager = Arc::new(manager);

                b.iter_custom(|iters| {
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|_| {
                            let manager = Arc::clone(&manager);
                            thread::spawn(move || {
                                for i in 0..ops_per_thread {
                                    let idx = i % 1000;
                                    black_box(manager.get_inode(&format!("shared/{}", idx)));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    // Mixed read-write (80% read, 20% write)
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("mixed_rw", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let manager = create_inode_manager();
                    // Pre-populate
                    for i in 0..10000 {
                        manager.get_or_create_inode(&format!("mixed/{}", i), false, 100);
                    }
                    let manager = Arc::new(manager);
                    let counter = Arc::new(AtomicU64::new(10000));
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|_| {
                            let manager = Arc::clone(&manager);
                            let counter = Arc::clone(&counter);
                            thread::spawn(move || {
                                for i in 0..ops_per_thread {
                                    if i % 5 == 0 {
                                        // 20% write
                                        let id = counter.fetch_add(1, Ordering::Relaxed);
                                        let path = format!("mixed/new{}", id);
                                        black_box(manager.get_or_create_inode(&path, false, 100));
                                    } else {
                                        // 80% read
                                        let idx = i % 10000;
                                        black_box(manager.get_inode(&format!("mixed/{}", idx)));
                                    }
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Concurrent Cache Benchmarks
// ============================================================================

pub fn cache_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_cache");

    // Concurrent metadata cache put
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("metadata_put", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let cache = create_metadata_cache();
                    let counter = Arc::new(AtomicU64::new(0));
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|_| {
                            let cache = Arc::clone(&cache);
                            let counter = Arc::clone(&counter);
                            thread::spawn(move || {
                                for _ in 0..ops_per_thread {
                                    let id = counter.fetch_add(1, Ordering::Relaxed);
                                    cache.put_attr(id, FileAttr::default());
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    // Concurrent metadata cache get (hot spot: 80% access 20% data)
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("metadata_get_hotspot", num_threads),
            &num_threads,
            |b, &num_threads| {
                let cache = create_metadata_cache();
                // Pre-populate
                for i in 0..10000 {
                    cache.put_attr(i, FileAttr::default());
                }

                b.iter_custom(|iters| {
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let cache = Arc::clone(&cache);
                            thread::spawn(move || {
                                let mut rng_state = t as u64;
                                for _ in 0..ops_per_thread {
                                    // Simple LCG for deterministic "random"
                                    rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                                    let r = (rng_state >> 16) % 100;
                                    let idx = if r < 80 {
                                        // 80% access hot 20% of data
                                        (rng_state >> 16) % 2000
                                    } else {
                                        // 20% access cold 80% of data
                                        2000 + (rng_state >> 16) % 8000
                                    };
                                    black_box(cache.get_attr(idx));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    // Concurrent data cache operations
    for &num_threads in THREAD_COUNTS {
        group.throughput(Throughput::Bytes(65536 * num_threads as u64));
        group.bench_with_input(
            BenchmarkId::new("data_put", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let cache = create_data_cache();
                    let counter = Arc::new(AtomicU64::new(0));
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let cache = Arc::clone(&cache);
                            let counter = Arc::clone(&counter);
                            let data = Bytes::from(vec![t as u8; 65536]);
                            thread::spawn(move || {
                                for _ in 0..ops_per_thread {
                                    let id = counter.fetch_add(1, Ordering::Relaxed);
                                    // Different inodes to avoid contention
                                    cache.put(id, 0, data.clone());
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    // Concurrent data cache get
    for &num_threads in THREAD_COUNTS {
        group.bench_with_input(
            BenchmarkId::new("data_get", num_threads),
            &num_threads,
            |b, &num_threads| {
                let cache = create_data_cache();
                let data = Bytes::from(vec![0u8; 65536]);
                // Pre-populate: 1000 blocks across 100 inodes
                for inode in 0..100u64 {
                    for block in 0..10u64 {
                        cache.put(inode, block * 65536, data.clone());
                    }
                }

                b.iter_custom(|iters| {
                    let ops_per_thread = iters as usize / num_threads;

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let cache = Arc::clone(&cache);
                            thread::spawn(move || {
                                for i in 0..ops_per_thread {
                                    let inode = ((t + i) % 100) as u64;
                                    let block = (i % 10) as u64;
                                    black_box(cache.get(inode, block * 65536));
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    start.elapsed()
                });
            },
        );
    }

    group.finish();
}
