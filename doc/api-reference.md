# API 参考文档

## 1. 命令行接口

### 1.1 obsfuse mount

挂载 OBS 存储桶到本地目录。

```bash
obsfuse mount <BUCKET> <MOUNTPOINT> [OPTIONS]
```

#### 参数

| 参数 | 描述 |
|-----|------|
| `BUCKET` | OBS 存储桶名称 |
| `MOUNTPOINT` | 本地挂载点路径 |

#### 选项

##### OBS 连接选项

| 选项 | 环境变量 | 默认值 | 描述 |
|-----|---------|-------|------|
| `--endpoint <URL>` | `OBS_ENDPOINT` | `obs.cn-north-1.myhuaweicloud.com` | OBS 端点 URL |
| `--access-key <KEY>` | `OBS_ACCESS_KEY` | - | 访问密钥 (必需) |
| `--secret-key <KEY>` | `OBS_SECRET_KEY` | - | 秘密密钥 (必需) |
| `--region <REGION>` | `OBS_REGION` | `cn-north-1` | OBS 区域 |
| `--prefix <PATH>` | - | - | 桶内前缀路径 |

##### 缓存选项

| 选项 | 默认值 | 描述 |
|-----|-------|------|
| `--cache-dir <DIR>` | 系统缓存目录 | 磁盘缓存目录 |
| `--memory-cache-size <SIZE>` | `512MB` | 内存缓存大小 |
| `--disk-cache-size <SIZE>` | `10GB` | 磁盘缓存大小 |
| `--metadata-ttl <SECS>` | `3` | 元数据 TTL (秒) |

##### 性能选项

| 选项 | 默认值 | 描述 |
|-----|-------|------|
| `--read-ahead` | `true` | 启用预读取 |
| `--write-buffer-size <SIZE>` | `64MB` | 写缓冲大小 |

##### 挂载选项

| 选项 | 默认值 | 描述 |
|-----|-------|------|
| `--allow-root` | `false` | 允许 root 访问 |
| `--allow-other` | `false` | 允许其他用户访问 |
| `--read-only` | `false` | 只读挂载 |
| `-f, --foreground` | `false` | 前台运行 |

##### 权限选项

| 选项 | 默认值 | 描述 |
|-----|-------|------|
| `--uid <UID>` | 当前用户 | 固定用户 ID |
| `--gid <GID>` | 当前组 | 固定组 ID |
| `--file-mode <MODE>` | `0644` | 文件权限模式 |
| `--dir-mode <MODE>` | `0755` | 目录权限模式 |

##### 其他选项

| 选项 | 默认值 | 描述 |
|-----|-------|------|
| `-c, --config <FILE>` | `~/.obsfuse/config.toml` | 配置文件路径 |
| `--log-level <LEVEL>` | `info` | 日志级别 (trace/debug/info/warn/error) |
| `--log-file <FILE>` | stderr | 日志文件路径 |

#### 示例

```bash
# 基本挂载
obsfuse mount my-bucket /mnt/obs

# 指定端点和凭证
obsfuse mount my-bucket /mnt/obs \
    --endpoint obs.cn-north-4.myhuaweicloud.com \
    --access-key AKID... \
    --secret-key SECRET...

# 只读挂载并允许其他用户访问
obsfuse mount my-bucket /mnt/obs --read-only --allow-other

# 自定义缓存设置
obsfuse mount my-bucket /mnt/obs \
    --memory-cache-size 1GB \
    --disk-cache-size 50GB \
    --metadata-ttl 5
```

### 1.2 obsfuse unmount

卸载已挂载的文件系统。

```bash
obsfuse unmount <MOUNTPOINT>
```

#### 示例

```bash
obsfuse unmount /mnt/obs
```

### 1.3 obsfuse version

显示版本信息。

```bash
obsfuse version
```

---

## 2. 配置文件格式

配置文件使用 TOML 格式，默认位置为 `~/.obsfuse/config.toml`。

### 2.1 完整配置示例

```toml
# OBS 连接配置
[obs]
endpoint = "obs.cn-north-1.myhuaweicloud.com"
bucket = "my-bucket"
region = "cn-north-1"
# access_key 和 secret_key 建议使用环境变量
# prefix = "data/"  # 可选，桶内前缀
max_connections = 64
request_timeout = "30s"
max_retries = 3
retry_delay = "500ms"

# 缓存配置
[cache]
memory_limit = "512MB"
disk_limit = "10GB"
cache_dir = "/tmp/obsfuse"
block_size = "4MB"

# 元数据缓存配置
[cache.metadata]
attr_ttl = "3s"
dir_ttl = "5s"
negative_ttl = "1s"
max_entries = 100000

# 性能配置
[performance]
read_ahead = true
read_ahead_window = "16MB"
read_concurrency = 4
write_buffer_size = "64MB"
multipart_threshold = "100MB"
multipart_part_size = "8MB"
multipart_concurrency = 5
flush_interval = "30s"

# FUSE 挂载配置
[fuse]
max_read = "4MB"
max_write = "4MB"
allow_root = false
allow_other = false
read_only = false
fs_name = "obsfuse"

# 权限配置
[permission]
mode = "fixed"  # "fixed" 或 "preserved"

[permission.fixed]
uid = 1000
gid = 1000
file_mode = "0644"
dir_mode = "0755"

# 日志配置
[logging]
level = "info"
# file = "/var/log/obsfuse.log"  # 可选，默认输出到 stderr
json = false
```

### 2.2 配置项说明

#### [obs] 部分

| 配置项 | 类型 | 必需 | 描述 |
|-------|------|-----|------|
| `endpoint` | string | 是 | OBS 服务端点 |
| `bucket` | string | 是 | 存储桶名称 |
| `region` | string | 否 | 区域标识 |
| `access_key` | string | 是* | 访问密钥 (*建议用环境变量) |
| `secret_key` | string | 是* | 秘密密钥 (*建议用环境变量) |
| `prefix` | string | 否 | 桶内路径前缀 |
| `max_connections` | int | 否 | 最大并发连接数 |
| `request_timeout` | duration | 否 | 请求超时时间 |
| `max_retries` | int | 否 | 最大重试次数 |
| `retry_delay` | duration | 否 | 重试间隔 |

#### [cache] 部分

| 配置项 | 类型 | 默认值 | 描述 |
|-------|------|-------|------|
| `memory_limit` | size | 512MB | 内存缓存上限 |
| `disk_limit` | size | 10GB | 磁盘缓存上限 |
| `cache_dir` | path | 系统缓存目录 | 磁盘缓存目录 |
| `block_size` | size | 4MB | 数据块大小 |

#### [cache.metadata] 部分

| 配置项 | 类型 | 默认值 | 描述 |
|-------|------|-------|------|
| `attr_ttl` | duration | 3s | 属性缓存 TTL |
| `dir_ttl` | duration | 5s | 目录缓存 TTL |
| `negative_ttl` | duration | 1s | 负缓存 TTL |
| `max_entries` | int | 100000 | 最大缓存条目数 |

#### [performance] 部分

| 配置项 | 类型 | 默认值 | 描述 |
|-------|------|-------|------|
| `read_ahead` | bool | true | 启用预读 |
| `read_ahead_window` | size | 16MB | 预读窗口大小 |
| `read_concurrency` | int | 4 | 并发读取数 |
| `write_buffer_size` | size | 64MB | 写缓冲大小 |
| `multipart_threshold` | size | 100MB | 分片上传阈值 |
| `multipart_part_size` | size | 8MB | 分片大小 |
| `multipart_concurrency` | int | 5 | 并发上传数 |
| `flush_interval` | duration | 30s | 自动刷新间隔 |

---

## 3. 环境变量

| 变量名 | 描述 |
|-------|------|
| `OBS_ACCESS_KEY` | OBS 访问密钥 |
| `OBS_SECRET_KEY` | OBS 秘密密钥 |
| `OBS_ENDPOINT` | OBS 端点 URL |
| `OBS_BUCKET` | 存储桶名称 |
| `OBS_REGION` | OBS 区域 |

---

## 4. 库 API

### 4.1 核心类型

```rust
// 创建文件系统
pub struct ObsFs { ... }

impl ObsFs {
    /// 创建新的 OBS 文件系统实例
    pub fn new(config: Config, metrics: Arc<Metrics>) -> Result<Self, ObsFuseError>;
}

// 配置
pub struct Config {
    pub obs: ObsConfig,
    pub cache: CacheConfig,
    pub performance: PerformanceConfig,
    pub fuse: FuseConfig,
    pub permission: PermissionConfig,
    pub logging: LoggingConfig,
}

impl Config {
    /// 从文件加载配置
    pub fn from_file(path: &PathBuf) -> Result<Self, ObsFuseError>;

    /// 从默认位置加载配置
    pub fn from_default_location() -> Result<Self, ObsFuseError>;

    /// 合并环境变量
    pub fn merge_env(&mut self);

    /// 验证配置
    pub fn validate(&self) -> Result<(), ObsFuseError>;
}

// 指标
pub struct Metrics { ... }

impl Metrics {
    /// 创建新的指标收集器
    pub fn new() -> Self;

    /// 获取摘要报告
    pub fn summary(&self) -> MetricsSummary;
}
```

### 4.2 使用示例

```rust
use obsfuse::{Config, Metrics, ObsFs};
use std::sync::Arc;
use fuse3::MountOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 加载配置
    let mut config = Config::from_default_location()?;
    config.obs.bucket = "my-bucket".to_string();
    config.merge_env();
    config.validate()?;

    // 创建指标收集器
    let metrics = Arc::new(Metrics::new());

    // 创建文件系统
    let fs = ObsFs::new(config, metrics.clone())?;

    // 挂载
    let mount_options = MountOptions::default().fs_name("obsfuse");
    let handle = fuse3::raw::Session::new(mount_options)
        .mount_with_unprivileged(fs, "/mnt/obs")
        .await?;

    // 等待
    handle.await?;

    // 打印指标
    println!("{}", metrics.summary());

    Ok(())
}
```

---

## 5. 错误码

| 错误 | errno | 描述 |
|-----|-------|------|
| `PathNotFound` | ENOENT | 路径不存在 |
| `InodeNotFound` | ENOENT | Inode 不存在 |
| `HandleNotFound` | EBADF | 无效的文件句柄 |
| `PermissionDenied` | EACCES | 权限拒绝 |
| `FileExists` | EEXIST | 文件已存在 |
| `NotADirectory` | ENOTDIR | 不是目录 |
| `IsADirectory` | EISDIR | 是目录 |
| `DirectoryNotEmpty` | ENOTEMPTY | 目录非空 |
| `InvalidArgument` | EINVAL | 无效参数 |
| `NotSupported` | ENOSYS | 操作不支持 |
| `Storage` | EIO | 存储错误 |
| `Config` | EINVAL | 配置错误 |
