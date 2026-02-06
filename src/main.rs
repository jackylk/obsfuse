//! OBS FUSE - High-performance FUSE filesystem for Huawei Cloud OBS
//!
//! This is the main entry point for the obsfuse command-line tool.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fuse3::MountOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use obsfuse::{Config, Metrics, ObsFs};

/// OBS FUSE - Mount Huawei Cloud OBS as a local filesystem
#[derive(Parser, Debug)]
#[command(name = "obsfuse")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Mount an OBS bucket
    Mount {
        /// OBS bucket name
        bucket: String,

        /// Mount point path
        mountpoint: PathBuf,

        /// OBS endpoint URL
        #[arg(long, env = "OBS_ENDPOINT")]
        endpoint: Option<String>,

        /// OBS access key
        #[arg(long, env = "OBS_ACCESS_KEY")]
        access_key: Option<String>,

        /// OBS secret key
        #[arg(long, env = "OBS_SECRET_KEY")]
        secret_key: Option<String>,

        /// OBS region
        #[arg(long, env = "OBS_REGION", default_value = "cn-north-1")]
        region: String,

        /// Prefix path within the bucket
        #[arg(long)]
        prefix: Option<String>,

        /// Configuration file path
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Cache directory
        #[arg(long)]
        cache_dir: Option<PathBuf>,

        /// Memory cache size (e.g., "512MB")
        #[arg(long)]
        memory_cache_size: Option<String>,

        /// Disk cache size (e.g., "10GB")
        #[arg(long)]
        disk_cache_size: Option<String>,

        /// Metadata TTL in seconds
        #[arg(long)]
        metadata_ttl: Option<u64>,

        /// Enable read-ahead
        #[arg(long)]
        read_ahead: Option<bool>,

        /// Write buffer size (e.g., "64MB")
        #[arg(long)]
        write_buffer_size: Option<String>,

        /// Allow root access
        #[arg(long)]
        allow_root: bool,

        /// Allow other users access
        #[arg(long)]
        allow_other: bool,

        /// Mount as read-only
        #[arg(long)]
        read_only: bool,

        /// Run in foreground
        #[arg(short, long)]
        foreground: bool,

        /// Log level (trace, debug, info, warn, error)
        #[arg(long, default_value = "info")]
        log_level: String,

        /// Log file path
        #[arg(long)]
        log_file: Option<PathBuf>,

        /// Fixed UID for all files
        #[arg(long)]
        uid: Option<u32>,

        /// Fixed GID for all files
        #[arg(long)]
        gid: Option<u32>,

        /// File permission mode (e.g., "0644")
        #[arg(long)]
        file_mode: Option<String>,

        /// Directory permission mode (e.g., "0755")
        #[arg(long)]
        dir_mode: Option<String>,
    },

    /// Unmount a mounted filesystem
    Unmount {
        /// Mount point path
        mountpoint: PathBuf,
    },

    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Mount {
            bucket,
            mountpoint,
            endpoint,
            access_key,
            secret_key,
            region,
            prefix,
            config: config_path,
            cache_dir,
            memory_cache_size,
            disk_cache_size,
            metadata_ttl,
            read_ahead,
            write_buffer_size,
            allow_root,
            allow_other,
            read_only,
            foreground: _,
            log_level,
            log_file,
            uid,
            gid,
            file_mode,
            dir_mode,
        } => {
            // Initialize logging
            init_logging(&log_level, log_file.as_ref())?;

            // Load configuration
            let mut config = if let Some(path) = config_path {
                Config::from_file(&path).context("Failed to load config file")?
            } else {
                Config::from_default_location().unwrap_or_default()
            };

            // Apply command-line overrides
            config.obs.bucket = bucket;
            if let Some(ep) = endpoint {
                config.obs.endpoint = ep;
            }
            if let Some(ak) = access_key {
                config.obs.access_key = Some(ak);
            }
            if let Some(sk) = secret_key {
                config.obs.secret_key = Some(sk);
            }
            config.obs.region = region;
            config.obs.prefix = prefix;

            if let Some(dir) = cache_dir {
                config.cache.cache_dir = Some(dir);
            }
            if let Some(size) = memory_cache_size {
                config.cache.memory_limit = size.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            }
            if let Some(size) = disk_cache_size {
                config.cache.disk_limit = size.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            }
            if let Some(ttl) = metadata_ttl {
                config.cache.metadata.attr_ttl = std::time::Duration::from_secs(ttl);
            }
            if let Some(ra) = read_ahead {
                config.performance.read_ahead = ra;
            }
            if let Some(size) = write_buffer_size {
                config.performance.write_buffer_size =
                    size.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            }

            config.fuse.allow_root = allow_root;
            config.fuse.allow_other = allow_other;
            config.fuse.read_only = read_only;

            if let Some(u) = uid {
                config.permission.fixed.uid = u;
            }
            if let Some(g) = gid {
                config.permission.fixed.gid = g;
            }
            if let Some(mode) = file_mode {
                config.permission.fixed.file_mode = parse_mode(&mode)?;
            }
            if let Some(mode) = dir_mode {
                config.permission.fixed.dir_mode = parse_mode(&mode)?;
            }

            // Merge environment variables
            config.merge_env();

            // Validate configuration
            config.validate().context("Invalid configuration")?;

            // Create metrics
            let metrics = Arc::new(Metrics::new());

            // Create filesystem
            let fs = ObsFs::new(config.clone(), metrics.clone())
                .context("Failed to create filesystem")?;

            // Ensure mount point exists
            if !mountpoint.exists() {
                std::fs::create_dir_all(&mountpoint)
                    .context("Failed to create mount point directory")?;
            }

            // Build mount options
            let mut mount_options = MountOptions::default();
            mount_options.fs_name(&config.fuse.fs_name);
            mount_options.read_only(config.fuse.read_only);

            if config.fuse.allow_root {
                mount_options.allow_root(true);
            }
            if config.fuse.allow_other {
                mount_options.allow_other(true);
            }

            info!(
                bucket = %config.obs.bucket,
                mountpoint = %mountpoint.display(),
                "Mounting OBS filesystem"
            );

            // Mount the filesystem
            let mount_handle = fuse3::raw::Session::new(mount_options)
                .mount_with_unprivileged(fs, &mountpoint)
                .await
                .context("Failed to mount filesystem")?;

            info!("Filesystem mounted successfully");

            // Handle signals
            let handle = mount_handle;
            tokio::select! {
                _ = signal::ctrl_c() => {
                    info!("Received interrupt signal, unmounting...");
                }
                result = handle => {
                    match result {
                        Ok(()) => info!("Filesystem unmounted"),
                        Err(e) => error!(error = %e, "Filesystem error"),
                    }
                }
            }

            info!("Shutdown complete");
        }

        Commands::Unmount { mountpoint } => {
            unmount(&mountpoint)?;
        }

        Commands::Version => {
            println!("obsfuse {}", env!("CARGO_PKG_VERSION"));
            println!("A high-performance FUSE filesystem for Huawei Cloud OBS");
        }
    }

    Ok(())
}

/// Initialize logging
fn init_logging(level: &str, log_file: Option<&PathBuf>) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    let subscriber = tracing_subscriber::registry().with(filter);

    if let Some(path) = log_file {
        let file = std::fs::File::create(path).context("Failed to create log file")?;
        let file_layer = fmt::layer()
            .with_writer(file)
            .with_ansi(false);
        subscriber.with(file_layer).init();
    } else {
        let stdout_layer = fmt::layer()
            .with_writer(std::io::stderr);
        subscriber.with(stdout_layer).init();
    }

    Ok(())
}

/// Parse permission mode string (e.g., "0644" or "644")
fn parse_mode(s: &str) -> Result<u32> {
    let s = s.trim_start_matches("0o").trim_start_matches("0");
    u32::from_str_radix(s, 8).context("Invalid permission mode")
}

/// Unmount a filesystem
fn unmount(mountpoint: &PathBuf) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let status = Command::new("fusermount")
            .arg("-u")
            .arg(mountpoint)
            .status()
            .context("Failed to run fusermount")?;

        if !status.success() {
            anyhow::bail!("fusermount failed with status: {}", status);
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let status = Command::new("umount")
            .arg(mountpoint)
            .status()
            .context("Failed to run umount")?;

        if !status.success() {
            anyhow::bail!("umount failed with status: {}", status);
        }
    }

    info!(mountpoint = %mountpoint.display(), "Filesystem unmounted");
    Ok(())
}
