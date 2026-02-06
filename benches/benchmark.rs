//! Comprehensive benchmark tests for OBS FUSE
//!
//! Run with:
//!   cargo bench --bench benchmark -- --quick    # Quick mode (~2 min)
//!   cargo bench --bench benchmark               # Normal mode (~10 min)
//!   cargo bench --bench benchmark -- --full     # Full mode (~30 min)
//!   cargo bench --bench benchmark -- inode      # Filter by name
//!
//! With OBS integration (requires credentials):
//!   source .obs-credentials
//!   cargo bench --bench benchmark --features obs-bench
//!
//! Generate reports:
//!   ./scripts/run-benchmark.sh

use criterion::{criterion_group, criterion_main, Criterion};
use std::time::Duration;

mod local_bench;
mod concurrent_bench;

#[cfg(feature = "obs-bench")]
mod obs_bench;

// Detect test mode from environment or command line
fn get_benchmark_config() -> Criterion {
    // Check for mode via environment variable (set by run-benchmark.sh)
    let mode = std::env::var("BENCH_MODE").unwrap_or_else(|_| "normal".to_string());

    match mode.as_str() {
        "quick" => Criterion::default()
            .sample_size(10)
            .measurement_time(Duration::from_secs(1))
            .warm_up_time(Duration::from_secs(1)),
        "full" => Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(10))
            .warm_up_time(Duration::from_secs(5)),
        _ => Criterion::default()  // normal
            .sample_size(50)
            .measurement_time(Duration::from_secs(5))
            .warm_up_time(Duration::from_secs(3)),
    }
}

// ============================================================================
// Local Component Benchmarks
// ============================================================================

fn benchmark_inode_operations(c: &mut Criterion) {
    local_bench::inode_benchmarks(c);
}

fn benchmark_cache_operations(c: &mut Criterion) {
    local_bench::cache_benchmarks(c);
}

fn benchmark_path_operations(c: &mut Criterion) {
    local_bench::path_benchmarks(c);
}

fn benchmark_readahead(c: &mut Criterion) {
    local_bench::readahead_benchmarks(c);
}

// ============================================================================
// Concurrent Benchmarks
// ============================================================================

fn benchmark_concurrent_inode(c: &mut Criterion) {
    concurrent_bench::inode_concurrent(c);
}

fn benchmark_concurrent_cache(c: &mut Criterion) {
    concurrent_bench::cache_concurrent(c);
}

// ============================================================================
// OBS Integration Benchmarks (optional)
// ============================================================================

#[cfg(feature = "obs-bench")]
fn benchmark_obs_operations(c: &mut Criterion) {
    obs_bench::obs_benchmarks(c);
}

#[cfg(feature = "obs-bench")]
fn benchmark_obs_concurrent(c: &mut Criterion) {
    obs_bench::obs_concurrent_benchmarks(c);
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(
    name = local_benches;
    config = get_benchmark_config();
    targets =
        benchmark_inode_operations,
        benchmark_cache_operations,
        benchmark_path_operations,
        benchmark_readahead,
);

criterion_group!(
    name = concurrent_benches;
    config = get_benchmark_config();
    targets =
        benchmark_concurrent_inode,
        benchmark_concurrent_cache,
);

#[cfg(feature = "obs-bench")]
criterion_group!(
    name = obs_benches;
    config = get_benchmark_config();
    targets =
        benchmark_obs_operations,
        benchmark_obs_concurrent,
);

#[cfg(feature = "obs-bench")]
criterion_main!(local_benches, concurrent_benches, obs_benches);

#[cfg(not(feature = "obs-bench"))]
criterion_main!(local_benches, concurrent_benches);
