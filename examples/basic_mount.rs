//! Basic mount example for OBS FUSE
//!
//! This example demonstrates how to programmatically mount an OBS bucket.

use anyhow::Result;
use fuse3::MountOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use obsfuse::{Config, Metrics, ObsFs};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(EnvFilter::new("info"))
        .with(fmt::layer())
        .init();

    // Create configuration
    let mut config = Config::default();

    // Set OBS credentials from environment
    config.obs.bucket = std::env::var("OBS_BUCKET").expect("OBS_BUCKET not set");
    config.obs.endpoint = std::env::var("OBS_ENDPOINT")
        .unwrap_or_else(|_| "obs.cn-north-1.myhuaweicloud.com".to_string());
    config.obs.access_key = Some(std::env::var("OBS_ACCESS_KEY").expect("OBS_ACCESS_KEY not set"));
    config.obs.secret_key = Some(std::env::var("OBS_SECRET_KEY").expect("OBS_SECRET_KEY not set"));

    // Validate configuration
    config.validate()?;

    // Create metrics collector
    let metrics = Arc::new(Metrics::new());

    // Create filesystem
    let fs = ObsFs::new(config.clone(), metrics.clone())?;

    // Mount point
    let mountpoint = PathBuf::from("/tmp/obs-mount");
    std::fs::create_dir_all(&mountpoint)?;

    // Mount options
    let mut mount_options = MountOptions::default();
    mount_options.fs_name("obsfuse");
    mount_options.read_only(false);

    info!("Mounting {} to {}", config.obs.bucket, mountpoint.display());

    // Mount the filesystem
    let mount_handle = fuse3::raw::Session::new(mount_options)
        .mount_with_unprivileged(fs, &mountpoint)
        .await?;

    info!("Filesystem mounted. Press Ctrl+C to unmount.");

    // Wait for interrupt
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Unmounting...");
        }
        result = mount_handle => {
            if let Err(e) = result {
                eprintln!("Mount error: {}", e);
            }
        }
    }

    // Print metrics summary
    println!("\n{}", metrics.summary());

    Ok(())
}
