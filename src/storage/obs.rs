//! OBS client wrapper using OpenDAL
//!
//! This module provides a high-level interface to Huawei Cloud OBS
//! using OpenDAL as the underlying storage abstraction.

use bytes::Bytes;
use opendal::Operator;
use std::sync::Arc;
use tracing::instrument;

use crate::config::ObsConfig;
use crate::utils::{Metrics, ObsFuseError, Result};

/// OBS client wrapper
pub struct ObsClient {
    /// OpenDAL operator
    operator: Operator,
    /// Configuration
    config: ObsConfig,
    /// Metrics collector
    metrics: Arc<Metrics>,
}

impl ObsClient {
    /// Create a new OBS client
    pub fn new(config: ObsConfig, metrics: Arc<Metrics>) -> Result<Self> {
        let operator = Self::build_operator(&config)?;

        Ok(Self {
            operator,
            config,
            metrics,
        })
    }

    /// Build OpenDAL operator for OBS
    fn build_operator(config: &ObsConfig) -> Result<Operator> {
        use opendal::services::Obs;

        let mut builder = Obs::default()
            .endpoint(&format!("https://{}", config.endpoint))
            .bucket(&config.bucket);

        if let Some(ref ak) = config.access_key {
            builder = builder.access_key_id(ak);
        }

        if let Some(ref sk) = config.secret_key {
            builder = builder.secret_access_key(sk);
        }

        // Build the operator with retry layer
        let op = Operator::new(builder)
            .map_err(|e| ObsFuseError::Storage(e))?
            .layer(opendal::layers::RetryLayer::new().with_max_times(config.max_retries as usize))
            .finish();

        Ok(op)
    }

    /// Get the underlying operator
    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    /// Check if a path exists
    #[instrument(skip(self), level = "debug")]
    pub async fn exists(&self, path: &str) -> Result<bool> {
        let result = self.operator.exists(path).await?;
        Ok(result)
    }

    /// Get object metadata
    #[instrument(skip(self), level = "debug")]
    pub async fn stat(&self, path: &str) -> Result<ObjectMeta> {
        self.metrics.inc_obs_get();

        let meta = self.operator.stat(path).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        Ok(ObjectMeta::from_opendal(path, &meta))
    }

    /// Read entire object
    #[instrument(skip(self), level = "debug")]
    pub async fn read(&self, path: &str) -> Result<Bytes> {
        self.metrics.inc_obs_get();

        let data = self.operator.read(path).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        self.metrics.add_read_bytes(data.len() as u64);
        Ok(data.to_bytes())
    }

    /// Read object with range
    #[instrument(skip(self), level = "debug")]
    pub async fn read_range(&self, path: &str, offset: u64, size: u64) -> Result<Bytes> {
        self.metrics.inc_obs_get();

        let data = self
            .operator
            .read_with(path)
            .range(offset..offset + size)
            .await
            .map_err(|e| {
                self.metrics.inc_obs_error();
                ObsFuseError::Storage(e)
            })?;

        self.metrics.add_read_bytes(data.len() as u64);
        Ok(data.to_bytes())
    }

    /// Write object
    #[instrument(skip(self, data), level = "debug")]
    pub async fn write(&self, path: &str, data: Bytes) -> Result<()> {
        self.metrics.inc_obs_put();

        let size = data.len() as u64;
        self.operator.write(path, data).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        self.metrics.add_write_bytes(size);
        Ok(())
    }

    /// Write object with metadata
    #[instrument(skip(self, data), level = "debug")]
    pub async fn write_with_meta(
        &self,
        path: &str,
        data: Bytes,
        meta: &ObjectWriteMeta,
    ) -> Result<()> {
        self.metrics.inc_obs_put();

        let size = data.len() as u64;
        let mut writer = self.operator.write_with(path, data);

        if let Some(content_type) = &meta.content_type {
            writer = writer.content_type(content_type);
        }

        writer.await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        self.metrics.add_write_bytes(size);
        Ok(())
    }

    /// Delete object
    #[instrument(skip(self), level = "debug")]
    pub async fn delete(&self, path: &str) -> Result<()> {
        self.metrics.inc_obs_delete();

        self.operator.delete(path).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        Ok(())
    }

    /// List objects with prefix
    #[instrument(skip(self), level = "debug")]
    pub async fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>> {
        self.metrics.inc_obs_list();

        let entries = self
            .operator
            .list_with(prefix)
            .await
            .map_err(|e| {
                self.metrics.inc_obs_error();
                ObsFuseError::Storage(e)
            })?;

        let mut results = Vec::new();
        for entry in entries {
            let meta = self.operator.stat(entry.path()).await.map_err(|e| {
                self.metrics.inc_obs_error();
                ObsFuseError::Storage(e)
            })?;
            results.push(ObjectMeta::from_opendal(entry.path(), &meta));
        }

        Ok(results)
    }

    /// List objects in directory (non-recursive)
    #[instrument(skip(self), level = "debug")]
    pub async fn list_dir(&self, prefix: &str) -> Result<Vec<ObjectMeta>> {
        self.metrics.inc_obs_list();

        // Ensure prefix ends with /
        let dir_prefix = if prefix.is_empty() || prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{}/", prefix)
        };

        let entries = self
            .operator
            .list_with(&dir_prefix)
            .await
            .map_err(|e| {
                self.metrics.inc_obs_error();
                ObsFuseError::Storage(e)
            })?;

        let mut results = Vec::new();
        for entry in entries {
            let meta = self.operator.stat(entry.path()).await.map_err(|e| {
                self.metrics.inc_obs_error();
                ObsFuseError::Storage(e)
            })?;
            results.push(ObjectMeta::from_opendal(entry.path(), &meta));
        }

        Ok(results)
    }

    /// Create directory (empty object with trailing /)
    #[instrument(skip(self), level = "debug")]
    pub async fn create_dir(&self, path: &str) -> Result<()> {
        self.metrics.inc_obs_put();

        // Ensure path ends with /
        let dir_path = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };

        self.operator.create_dir(&dir_path).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        Ok(())
    }

    /// Copy object
    #[instrument(skip(self), level = "debug")]
    pub async fn copy(&self, from: &str, to: &str) -> Result<()> {
        self.metrics.inc_obs_get();
        self.metrics.inc_obs_put();

        self.operator.copy(from, to).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        Ok(())
    }

    /// Rename/move object
    #[instrument(skip(self), level = "debug")]
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.metrics.inc_obs_get();
        self.metrics.inc_obs_put();
        self.metrics.inc_obs_delete();

        self.operator.rename(from, to).await.map_err(|e| {
            self.metrics.inc_obs_error();
            ObsFuseError::Storage(e)
        })?;

        Ok(())
    }

    /// Get configuration
    pub fn config(&self) -> &ObsConfig {
        &self.config
    }
}

/// Object metadata
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    /// Object path/key
    pub path: String,
    /// Object size in bytes
    pub size: u64,
    /// Last modified time
    pub last_modified: Option<std::time::SystemTime>,
    /// Is directory
    pub is_dir: bool,
    /// Content type
    pub content_type: Option<String>,
    /// ETag
    pub etag: Option<String>,
}

impl ObjectMeta {
    /// Create from OpenDAL metadata
    fn from_opendal(path: &str, meta: &opendal::Metadata) -> Self {
        Self {
            path: path.to_string(),
            size: meta.content_length(),
            last_modified: meta.last_modified().map(|t| {
                // opendal::raw::Timestamp implements Into<SystemTime>
                t.into()
            }),
            is_dir: meta.is_dir(),
            content_type: meta.content_type().map(|s| s.to_string()),
            etag: meta.etag().map(|s| s.to_string()),
        }
    }

    /// Get file name from path
    pub fn name(&self) -> &str {
        let path = self.path.trim_end_matches('/');
        path.rsplit('/').next().unwrap_or(path)
    }
}

/// Metadata for write operations
#[derive(Debug, Clone, Default)]
pub struct ObjectWriteMeta {
    /// Content type
    pub content_type: Option<String>,
    /// Custom metadata (x-obs-meta-*)
    pub custom_meta: std::collections::HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_meta_name() {
        let meta = ObjectMeta {
            path: "path/to/file.txt".to_string(),
            size: 100,
            last_modified: None,
            is_dir: false,
            content_type: None,
            etag: None,
        };
        assert_eq!(meta.name(), "file.txt");

        let dir_meta = ObjectMeta {
            path: "path/to/dir/".to_string(),
            size: 0,
            last_modified: None,
            is_dir: true,
            content_type: None,
            etag: None,
        };
        assert_eq!(dir_meta.name(), "dir");
    }
}
