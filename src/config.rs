//! Configuration management for OBS FUSE filesystem
//!
//! This module handles configuration from multiple sources:
//! - Command line arguments
//! - Configuration file (~/.obsfuse/config.toml)
//! - Environment variables

use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::utils::ObsFuseError;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// OBS connection settings
    pub obs: ObsConfig,

    /// Cache settings
    pub cache: CacheConfig,

    /// Performance tuning
    pub performance: PerformanceConfig,

    /// FUSE mount options
    pub fuse: FuseConfig,

    /// Permission settings
    pub permission: PermissionConfig,

    /// Logging settings
    pub logging: LoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            obs: ObsConfig::default(),
            cache: CacheConfig::default(),
            performance: PerformanceConfig::default(),
            fuse: FuseConfig::default(),
            permission: PermissionConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

/// OBS connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsConfig {
    /// OBS endpoint URL
    pub endpoint: String,

    /// Bucket name
    pub bucket: String,

    /// Region
    pub region: String,

    /// Access key (prefer environment variable OBS_ACCESS_KEY)
    #[serde(skip_serializing)]
    pub access_key: Option<String>,

    /// Secret key (prefer environment variable OBS_SECRET_KEY)
    #[serde(skip_serializing)]
    pub secret_key: Option<String>,

    /// Prefix path within the bucket (optional)
    pub prefix: Option<String>,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// Request timeout
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,

    /// Maximum retry attempts
    pub max_retries: u32,

    /// Retry delay
    #[serde(with = "humantime_serde")]
    pub retry_delay: Duration,
}

impl Default for ObsConfig {
    fn default() -> Self {
        Self {
            endpoint: "obs.cn-north-1.myhuaweicloud.com".to_string(),
            bucket: String::new(),
            region: "cn-north-1".to_string(),
            access_key: None,
            secret_key: None,
            prefix: None,
            max_connections: 64,
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// Memory cache size limit
    pub memory_limit: ByteSize,

    /// Disk cache size limit
    pub disk_limit: ByteSize,

    /// Cache directory
    pub cache_dir: Option<PathBuf>,

    /// Block size for data cache
    pub block_size: ByteSize,

    /// Metadata cache settings
    pub metadata: MetadataCacheConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            memory_limit: ByteSize::mb(512),
            disk_limit: ByteSize::gb(10),
            cache_dir: None,
            block_size: ByteSize::mb(4),
            metadata: MetadataCacheConfig::default(),
        }
    }
}

/// Metadata cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetadataCacheConfig {
    /// Attribute cache TTL
    #[serde(with = "humantime_serde")]
    pub attr_ttl: Duration,

    /// Directory listing cache TTL
    #[serde(with = "humantime_serde")]
    pub dir_ttl: Duration,

    /// Negative cache TTL (for non-existent paths)
    #[serde(with = "humantime_serde")]
    pub negative_ttl: Duration,

    /// Maximum number of cached entries
    pub max_entries: usize,
}

impl Default for MetadataCacheConfig {
    fn default() -> Self {
        Self {
            attr_ttl: Duration::from_secs(3),
            dir_ttl: Duration::from_secs(5),
            negative_ttl: Duration::from_secs(1),
            max_entries: 100_000,
        }
    }
}

/// Performance tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Enable read-ahead
    pub read_ahead: bool,

    /// Read-ahead window size
    pub read_ahead_window: ByteSize,

    /// Read concurrency (parallel downloads)
    pub read_concurrency: usize,

    /// Write buffer size per file
    pub write_buffer_size: ByteSize,

    /// Threshold for multipart upload
    pub multipart_threshold: ByteSize,

    /// Part size for multipart upload
    pub multipart_part_size: ByteSize,

    /// Concurrency for multipart upload
    pub multipart_concurrency: usize,

    /// Flush interval for write buffer
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            read_ahead: true,
            read_ahead_window: ByteSize::mb(16),
            read_concurrency: 4,
            write_buffer_size: ByteSize::mb(64),
            multipart_threshold: ByteSize::mb(100),
            multipart_part_size: ByteSize::mb(8),
            multipart_concurrency: 5,
            flush_interval: Duration::from_secs(30),
        }
    }
}

/// FUSE mount configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FuseConfig {
    /// Maximum read size
    pub max_read: ByteSize,

    /// Maximum write size
    pub max_write: ByteSize,

    /// Allow root access
    pub allow_root: bool,

    /// Allow other users access
    pub allow_other: bool,

    /// Mount as read-only
    pub read_only: bool,

    /// Enable direct I/O (bypass page cache)
    pub direct_io: bool,

    /// Filesystem name
    pub fs_name: String,
}

impl Default for FuseConfig {
    fn default() -> Self {
        Self {
            max_read: ByteSize::mb(4),
            max_write: ByteSize::mb(4),
            allow_root: false,
            allow_other: false,
            read_only: false,
            direct_io: false,
            fs_name: "obsfuse".to_string(),
        }
    }
}

/// Permission configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionConfig {
    /// Permission mode: "fixed" or "preserved"
    pub mode: PermissionMode,

    /// Fixed mode settings
    pub fixed: FixedPermission,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Fixed,
            fixed: FixedPermission::default(),
        }
    }
}

/// Permission mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Fixed permissions for all files
    Fixed,
    /// Preserve permissions in OBS metadata
    Preserved,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Fixed
    }
}

/// Fixed permission settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FixedPermission {
    /// User ID
    pub uid: u32,

    /// Group ID
    pub gid: u32,

    /// File mode (e.g., 0o644)
    pub file_mode: u32,

    /// Directory mode (e.g., 0o755)
    pub dir_mode: u32,
}

impl Default for FixedPermission {
    fn default() -> Self {
        Self {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            file_mode: 0o644,
            dir_mode: 0o755,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level
    pub level: String,

    /// Log file path (optional)
    pub file: Option<PathBuf>,

    /// Enable JSON format
    pub json: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: None,
            json: false,
        }
    }
}

impl Config {
    /// Load configuration from file
    pub fn from_file(path: &PathBuf) -> Result<Self, ObsFuseError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ObsFuseError::Config(format!("Failed to read config file: {}", e)))?;

        toml::from_str(&content)
            .map_err(|e| ObsFuseError::Config(format!("Failed to parse config file: {}", e)))
    }

    /// Load configuration from default location
    pub fn from_default_location() -> Result<Self, ObsFuseError> {
        let config_path = Self::default_config_path();
        if config_path.exists() {
            Self::from_file(&config_path)
        } else {
            Ok(Self::default())
        }
    }

    /// Get default configuration file path
    pub fn default_config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".obsfuse")
            .join("config.toml")
    }

    /// Get default cache directory
    pub fn default_cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("obsfuse")
    }

    /// Merge with environment variables
    pub fn merge_env(&mut self) {
        if let Ok(ak) = std::env::var("OBS_ACCESS_KEY") {
            self.obs.access_key = Some(ak);
        }
        if let Ok(sk) = std::env::var("OBS_SECRET_KEY") {
            self.obs.secret_key = Some(sk);
        }
        if let Ok(endpoint) = std::env::var("OBS_ENDPOINT") {
            self.obs.endpoint = endpoint;
        }
        if let Ok(bucket) = std::env::var("OBS_BUCKET") {
            self.obs.bucket = bucket;
        }
        if let Ok(region) = std::env::var("OBS_REGION") {
            self.obs.region = region;
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), ObsFuseError> {
        if self.obs.bucket.is_empty() {
            return Err(ObsFuseError::Config("Bucket name is required".to_string()));
        }

        if self.obs.access_key.is_none() {
            return Err(ObsFuseError::Config(
                "Access key is required (set OBS_ACCESS_KEY environment variable)".to_string(),
            ));
        }

        if self.obs.secret_key.is_none() {
            return Err(ObsFuseError::Config(
                "Secret key is required (set OBS_SECRET_KEY environment variable)".to_string(),
            ));
        }

        if self.cache.block_size.as_u64() == 0 {
            return Err(ObsFuseError::Config(
                "Block size must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Get effective cache directory
    pub fn effective_cache_dir(&self) -> PathBuf {
        self.cache
            .cache_dir
            .clone()
            .unwrap_or_else(Self::default_cache_dir)
    }
}

/// Humantime serde module for Duration serialization
mod humantime_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = humantime::format_duration(*duration).to_string();
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.cache.memory_limit, ByteSize::mb(512));
        assert_eq!(config.performance.read_ahead, true);
        assert_eq!(config.permission.mode, PermissionMode::Fixed);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        config.obs.bucket = "test-bucket".to_string();
        config.obs.access_key = Some("ak".to_string());
        config.obs.secret_key = Some("sk".to_string());

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_missing_bucket() {
        let config = Config::default();
        assert!(config.validate().is_err());
    }
}
