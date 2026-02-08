//! OBS FUSE - High-performance FUSE filesystem for Huawei Cloud OBS
//!
//! This is the main entry point for the obsfuse command-line tool.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use obsfuse::{Config, Metrics};

#[cfg(unix)]
use obsfuse::ObsFs;

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
            config.obs.region = region;
            if let Some(ep) = endpoint {
                config.obs.endpoint = ep;
            } else {
                // Derive endpoint from region if not explicitly provided
                config.obs.endpoint = format!("obs.{}.myhuaweicloud.com", config.obs.region);
            }
            if let Some(ak) = access_key {
                config.obs.access_key = Some(ak);
            }
            if let Some(sk) = secret_key {
                config.obs.secret_key = Some(sk);
            }
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

            // Mount filesystem (platform-specific)
            #[cfg(unix)]
            {
                mount_unix(config, metrics, &mountpoint).await?;
            }

            #[cfg(windows)]
            {
                let mountpoint_str = mountpoint.to_string_lossy().to_string();
                obsfuse::fs::mount_winfsp(config, metrics, &mountpoint_str).await?;
            }

            info!("Shutdown complete");
        }

        Commands::Unmount { mountpoint } => {
            unmount(&mountpoint)?;
        }

        Commands::Version => {
            println!("obsfuse {}", env!("CARGO_PKG_VERSION"));
            println!("A high-performance FUSE filesystem for Huawei Cloud OBS");
            #[cfg(unix)]
            println!("Platform: Unix (FUSE)");
            #[cfg(windows)]
            println!("Platform: Windows (WinFSP)");
        }
    }

    Ok(())
}

/// Mount filesystem on Unix using FUSE
#[cfg(unix)]
async fn mount_unix(config: Config, metrics: Arc<Metrics>, mountpoint: &PathBuf) -> Result<()> {
    use fuse3::MountOptions;
    use tokio::signal;

    // Install panic hook to clean up stale mount on panic
    let panic_mountpoint = mountpoint.clone();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("obsfuse panicked, cleaning up mount at {}", panic_mountpoint.display());
        force_unmount(&panic_mountpoint);
        prev_hook(info);
    }));

    // Create filesystem
    let fs = ObsFs::new(config.clone(), metrics.clone())
        .context("Failed to create filesystem")?;

    // Clean up stale mount if present
    cleanup_stale_mount(mountpoint);

    // Ensure mount point exists
    if !mountpoint.exists() {
        std::fs::create_dir_all(mountpoint)
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
        .mount_with_unprivileged(fs, mountpoint)
        .await
        .context("Failed to mount filesystem")?;

    info!("Filesystem mounted successfully");

    // Wait for ctrl_c, then explicitly unmount.
    // If the FUSE session ends on its own (external umount or error),
    // mount_handle.unmount() will still clean up properly.
    signal::ctrl_c().await.ok();
    info!("Received interrupt signal, unmounting...");

    if let Err(e) = mount_handle.unmount().await {
        error!(error = %e, "Clean unmount failed, forcing cleanup");
        force_unmount(mountpoint);
    } else {
        info!("Filesystem unmounted cleanly");
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

/// Force unmount a mountpoint using platform-specific commands
#[cfg(unix)]
fn force_unmount(mountpoint: &PathBuf) {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("diskutil")
            .args(["unmount", "force"])
            .arg(mountpoint)
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("fusermount")
            .args(["-u", "-z"]) // -z for lazy unmount
            .arg(mountpoint)
            .output();
    }
}

/// Check if a mountpoint has a stale FUSE mount and clean it up
#[cfg(unix)]
fn cleanup_stale_mount(mountpoint: &PathBuf) {
    use tracing::warn;

    // Check if the mountpoint exists and is a stale mount
    if !mountpoint.exists() {
        return;
    }

    // Try to stat the mountpoint - if it fails with "Device not configured" or similar,
    // it's a stale mount
    match std::fs::read_dir(mountpoint) {
        Ok(_) => return, // Mountpoint is accessible, not stale
        Err(e) => {
            let raw_error = e.raw_os_error();
            // ENXIO (6) = "Device not configured" on macOS
            // EIO (5) = "Input/output error"
            // ENOTCONN (57) = "Socket is not connected" (sometimes seen with FUSE)
            if raw_error != Some(libc::ENXIO)
                && raw_error != Some(libc::EIO)
                && raw_error != Some(libc::ENOTCONN)
            {
                return;
            }
            warn!(
                mountpoint = %mountpoint.display(),
                error = %e,
                "Detected stale mount, attempting cleanup"
            );
        }
    }

    force_unmount(mountpoint);

    // Verify cleanup succeeded
    match std::fs::read_dir(mountpoint) {
        Ok(_) => info!(mountpoint = %mountpoint.display(), "Stale mount cleaned up successfully"),
        Err(_) => warn!(mountpoint = %mountpoint.display(), "Failed to clean up stale mount"),
    }
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

    #[cfg(windows)]
    {
        // On Windows, unmount is handled by WinFSP when the process exits
        // For explicit unmount, we would need to signal the running process
        info!(mountpoint = %mountpoint.display(), "Windows: Unmount by terminating the mount process");
    }

    info!(mountpoint = %mountpoint.display(), "Filesystem unmounted");
    Ok(())
}
