//! OBS integration benchmarks (requires network and credentials)
//!
//! Enable with: cargo bench --features obs-bench

use bytes::Bytes;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use opendal::services::Obs;
use opendal::Operator;
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use tokio::runtime::Runtime;

const TEST_PREFIX: &str = "obsfuse-bench";
const CONCURRENT_LEVELS: &[usize] = &[1, 4, 8, 16, 32];

fn create_operator() -> Option<Operator> {
    let ak = env::var("OBS_ACCESS_KEY").ok()?;
    let sk = env::var("OBS_SECRET_KEY").ok()?;

    let builder = Obs::default()
        .endpoint("https://obs.cn-north-4.myhuaweicloud.com")
        .bucket("obs-fs-test-jska")
        .access_key_id(&ak)
        .secret_access_key(&sk);

    Operator::new(builder).ok().map(|op| op.finish())
}

fn test_path(name: &str) -> String {
    format!("{}/{}", TEST_PREFIX, name)
}

// ============================================================================
// OBS Single Operation Benchmarks
// ============================================================================

pub fn obs_benchmarks(c: &mut Criterion) {
    let Some(op) = create_operator() else {
        eprintln!("Skipping OBS benchmarks: credentials not set");
        return;
    };

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("obs");

    // Cleanup old test files first
    rt.block_on(async {
        let _ = op.remove_all(&format!("{}/", TEST_PREFIX)).await;
    });

    // Write benchmarks with different sizes
    let sizes: Vec<(usize, &str)> = vec![
        (1024, "1KB"),
        (64 * 1024, "64KB"),
        (1024 * 1024, "1MB"),
        (10 * 1024 * 1024, "10MB"),
    ];

    for (size, label) in &sizes {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(
            BenchmarkId::new("write", label),
            size,
            |b, &size| {
                let data = Bytes::from(vec![0xABu8; size]);
                let counter = AtomicU64::new(0);

                b.iter(|| {
                    let id = counter.fetch_add(1, Ordering::Relaxed);
                    let path = test_path(&format!("write_{}_{}", label, id));
                    rt.block_on(async {
                        black_box(op.write(&path, data.clone()).await.unwrap());
                    });
                });
            },
        );
    }

    // Prepare files for read benchmarks
    rt.block_on(async {
        for (size, label) in &sizes {
            let data = Bytes::from(vec![0xCDu8; *size]);
            let path = test_path(&format!("read_{}", label));
            op.write(&path, data).await.unwrap();
        }
    });

    // Read benchmarks
    for (size, label) in &sizes {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(
            BenchmarkId::new("read", label),
            size,
            |b, _| {
                let path = test_path(&format!("read_{}", label));
                b.iter(|| {
                    rt.block_on(async {
                        black_box(op.read(&path).await.unwrap());
                    });
                });
            },
        );
    }

    // Range read benchmark
    group.throughput(Throughput::Bytes(64 * 1024));
    group.bench_function("read_range_64KB", |b| {
        let path = test_path("read_10MB");
        let mut offset = 0u64;
        b.iter(|| {
            offset = (offset + 65536) % (9 * 1024 * 1024); // Stay within file
            rt.block_on(async {
                black_box(
                    op.read_with(&path)
                        .range(offset..offset + 65536)
                        .await
                        .unwrap()
                );
            });
        });
    });

    // Stat benchmark
    group.bench_function("stat", |b| {
        let path = test_path("read_1MB");
        b.iter(|| {
            rt.block_on(async {
                black_box(op.stat(&path).await.unwrap());
            });
        });
    });

    // Exists benchmark
    group.bench_function("exists", |b| {
        let path = test_path("read_1MB");
        b.iter(|| {
            rt.block_on(async {
                black_box(op.exists(&path).await.unwrap());
            });
        });
    });

    // Mkdir benchmark
    group.bench_function("mkdir", |b| {
        let counter = AtomicU64::new(0);

        b.iter(|| {
            let id = counter.fetch_add(1, Ordering::Relaxed);
            let path = test_path(&format!("mkdir_bench_{}/", id));
            rt.block_on(async {
                black_box(op.create_dir(&path).await.unwrap());
            });
        });
    });

    // List benchmark - prepare directory with files
    rt.block_on(async {
        for i in 0..100 {
            let path = test_path(&format!("list_dir/file_{}.txt", i));
            op.write(&path, "test").await.unwrap();
        }
    });

    group.bench_function("list_100", |b| {
        let prefix = test_path("list_dir/");
        b.iter(|| {
            rt.block_on(async {
                black_box(op.list(&prefix).await.unwrap());
            });
        });
    });

    // Delete benchmark
    group.bench_function("delete", |b| {
        let counter = AtomicU64::new(0);

        b.iter_batched(
            || {
                // Setup: create file to delete
                let id = counter.fetch_add(1, Ordering::Relaxed);
                let path = test_path(&format!("delete_{}", id));
                rt.block_on(async {
                    op.write(&path, "to delete").await.unwrap();
                });
                path
            },
            |path| {
                rt.block_on(async {
                    black_box(op.delete(&path).await.unwrap());
                });
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Copy benchmark
    group.bench_function("copy", |b| {
        let counter = AtomicU64::new(0);
        let src_path = test_path("read_1MB");

        b.iter(|| {
            let id = counter.fetch_add(1, Ordering::Relaxed);
            let dst_path = test_path(&format!("copy_dst_{}", id));
            rt.block_on(async {
                black_box(op.copy(&src_path, &dst_path).await.unwrap());
            });
        });
    });

    group.finish();
}

// ============================================================================
// OBS Concurrent Benchmarks
// ============================================================================

pub fn obs_concurrent_benchmarks(c: &mut Criterion) {
    let Some(op) = create_operator() else {
        eprintln!("Skipping OBS concurrent benchmarks: credentials not set");
        return;
    };

    let op = Arc::new(op);
    let rt = Arc::new(Runtime::new().unwrap());
    let mut group = c.benchmark_group("obs_concurrent");
    group.sample_size(10); // Fewer samples for network tests

    // Prepare test files for concurrent read
    rt.block_on(async {
        let data = Bytes::from(vec![0xEFu8; 1024 * 1024]); // 1MB
        for i in 0..32 {
            let path = test_path(&format!("concurrent_read_{}", i));
            op.write(&path, data.clone()).await.unwrap();
        }
    });

    // Concurrent read
    for &num_threads in CONCURRENT_LEVELS {
        group.throughput(Throughput::Bytes(1024 * 1024 * num_threads as u64));
        group.bench_with_input(
            BenchmarkId::new("read_1MB", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter_custom(|iters| {
                    let ops_per_thread = (iters as usize / num_threads).max(1);

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let op = Arc::clone(&op);
                            let rt = Arc::clone(&rt);
                            thread::spawn(move || {
                                for i in 0..ops_per_thread {
                                    let file_idx = (t + i) % 32;
                                    let path = test_path(&format!("concurrent_read_{}", file_idx));
                                    rt.block_on(async {
                                        black_box(op.read(&path).await.unwrap());
                                    });
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

    // Concurrent write
    for &num_threads in CONCURRENT_LEVELS {
        group.throughput(Throughput::Bytes(1024 * 1024 * num_threads as u64));
        group.bench_with_input(
            BenchmarkId::new("write_1MB", num_threads),
            &num_threads,
            |b, &num_threads| {
                let counter = Arc::new(AtomicU64::new(0));

                b.iter_custom(|iters| {
                    let ops_per_thread = (iters as usize / num_threads).max(1);

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let op = Arc::clone(&op);
                            let rt = Arc::clone(&rt);
                            let counter = Arc::clone(&counter);
                            let data = Bytes::from(vec![t as u8; 1024 * 1024]);
                            thread::spawn(move || {
                                for _ in 0..ops_per_thread {
                                    let id = counter.fetch_add(1, Ordering::Relaxed);
                                    let path = test_path(&format!("concurrent_write_{}", id));
                                    rt.block_on(async {
                                        black_box(op.write(&path, data.clone()).await.unwrap());
                                    });
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

    // Mixed read-write workload
    for &num_threads in CONCURRENT_LEVELS {
        group.bench_with_input(
            BenchmarkId::new("mixed_rw", num_threads),
            &num_threads,
            |b, &num_threads| {
                let counter = Arc::new(AtomicU64::new(0));

                b.iter_custom(|iters| {
                    let ops_per_thread = (iters as usize / num_threads).max(1);

                    let start = Instant::now();
                    let handles: Vec<_> = (0..num_threads)
                        .map(|t| {
                            let op = Arc::clone(&op);
                            let rt = Arc::clone(&rt);
                            let counter = Arc::clone(&counter);
                            thread::spawn(move || {
                                let data = Bytes::from(vec![t as u8; 65536]); // 64KB
                                for i in 0..ops_per_thread {
                                    if i % 5 == 0 {
                                        // 20% write
                                        let id = counter.fetch_add(1, Ordering::Relaxed);
                                        let path = test_path(&format!("mixed_write_{}", id));
                                        rt.block_on(async {
                                            black_box(op.write(&path, data.clone()).await.unwrap());
                                        });
                                    } else {
                                        // 80% read
                                        let file_idx = (t + i) % 32;
                                        let path = test_path(&format!("concurrent_read_{}", file_idx));
                                        rt.block_on(async {
                                            black_box(op.read(&path).await.unwrap());
                                        });
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

    // Cleanup
    rt.block_on(async {
        let _ = op.remove_all(&format!("{}/", TEST_PREFIX)).await;
    });
}
