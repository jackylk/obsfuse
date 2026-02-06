# OBS FUSE 架构设计文档

## 1. 项目概述

OBS FUSE 是一个基于 Rust 的高性能 FUSE 文件系统，用于挂载华为云 OBS 对象存储，使其像本地文件系统一样使用。

### 1.1 技术选型

| 组件 | 选择 | 理由 |
|-----|------|------|
| **语言** | Rust | 内存安全、高性能、零成本抽象 |
| **FUSE库** | fuse3 | 异步优先设计，更适合I/O密集型操作 |
| **OBS SDK** | OpenDAL | 原生支持华为OBS，统一存储抽象 |
| **异步运行时** | Tokio | 成熟的异步生态，高性能 |

### 1.2 设计目标

- **使用场景**: 读写均衡 + 混合文件大小 + 强一致性
- **目标平台**: Linux + macOS + 容器化支持
- **权限模型**: 可配置 (固定权限/保留权限)
- **扩展属性**: 不支持

---

## 2. 整体架构

```
┌────────────────────────────────────────────────────────────┐
│                    应用程序 (用户空间)                        │
└───────────────────────────┬────────────────────────────────┘
                            │ POSIX API
┌───────────────────────────▼────────────────────────────────┐
│                    Linux VFS / macOS VFS                    │
└───────────────────────────┬────────────────────────────────┘
                            │ FUSE协议
┌───────────────────────────▼────────────────────────────────┐
│              FUSE层 (fuse3 异步接口)                         │
│  • 异步请求处理                                              │
│  • Writeback-cache模式                                      │
│  • 并发操作调度                                              │
└───────────────────────────┬────────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────────┐
│              文件系统逻辑层 (ObsFs)                           │
│  • Inode管理 (路径 <-> inode映射)                           │
│  • 目录结构管理                                              │
│  • 权限与属性处理                                            │
│  • 文件句柄管理                                              │
└───────────────────────────┬────────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────────┐
│                    缓存层                                    │
│  ┌─────────────────────┬─────────────────────┐             │
│  │   元数据缓存         │   数据缓存           │             │
│  │  (DashMap+TTL)      │  (内存+磁盘LRU)     │             │
│  └─────────────────────┴─────────────────────┘             │
│  • 预读缓存 (Readahead Buffer)                              │
│  • 写缓冲区 (Write Buffer)                                  │
└───────────────────────────┬────────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────────┐
│              对象存储抽象层 (OpenDAL)                         │
│  • 连接池管理                                                │
│  • 分片上传/下载                                             │
│  • 重试与错误处理                                            │
│  • 并发请求控制                                              │
└───────────────────────────┬────────────────────────────────┘
                            │ HTTPS
┌───────────────────────────▼────────────────────────────────┐
│                    华为云 OBS                                │
└────────────────────────────────────────────────────────────┘
```

---

## 3. 模块设计

### 3.1 项目结构

```
obsfuse/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口点,命令行解析
│   ├── lib.rs               # 库导出
│   ├── config.rs            # 配置管理
│   ├── fs/
│   │   ├── mod.rs           # 文件系统模块
│   │   ├── obsfs.rs         # 主文件系统实现(实现fuse3 Filesystem trait)
│   │   ├── inode.rs         # Inode管理
│   │   ├── handle.rs        # 文件句柄管理
│   │   ├── attr.rs          # 文件属性处理
│   │   ├── permission.rs    # 权限管理
│   │   └── dir.rs           # 目录操作
│   ├── cache/
│   │   ├── mod.rs           # 缓存模块
│   │   ├── metadata.rs      # 元数据缓存
│   │   ├── data.rs          # 数据块缓存
│   │   ├── readahead.rs     # 预读缓存
│   │   └── write_buffer.rs  # 写缓冲
│   ├── storage/
│   │   ├── mod.rs           # 存储抽象层
│   │   ├── obs.rs           # OBS客户端封装
│   │   ├── multipart.rs     # 分片上传
│   │   └── retry.rs         # 重试逻辑
│   └── utils/
│       ├── mod.rs           # 工具模块
│       ├── error.rs         # 错误类型
│       └── metrics.rs       # 性能指标
├── tests/
│   ├── integration/         # 集成测试
│   └── unit/                # 单元测试
├── benches/                 # 性能基准测试
└── examples/                # 使用示例
```

### 3.2 Inode管理模块 (`src/fs/inode.rs`)

**设计目标**: 为OBS对象提供稳定的inode编号

```rust
pub struct InodeManager {
    /// 路径到inode的映射
    path_to_inode: DashMap<String, u64>,
    /// inode到元数据的映射
    inode_to_entry: DashMap<u64, InodeEntry>,
    /// inode计数器
    next_inode: AtomicU64,
    /// 权限配置
    permission_config: PermissionConfig,
}

pub struct InodeEntry {
    pub path: String,
    pub attr: FileAttr,
    pub is_dir: bool,
    pub children: Option<Vec<u64>>,
    pub cached_at: Instant,
    pub ref_count: u64,
}
```

**关键功能**:
- 路径到inode的双向映射
- Inode号生成 (递增序号确保唯一性)
- 支持目录层级结构
- TTL过期管理
- 引用计数管理

### 3.3 文件句柄管理 (`src/fs/handle.rs`)

```rust
pub struct HandleManager {
    handles: DashMap<u64, HandleState>,
    next_handle: AtomicU64,
}

pub struct HandleState {
    pub inode: u64,
    pub flags: u32,
    pub writable: bool,
    pub readable: bool,
    pub position: u64,
    pub last_access: Instant,
    pub dirty: bool,
    pub is_dir: bool,
}
```

**功能**:
- 管理打开的文件/目录句柄
- 跟踪读写位置
- 脏数据标记

---

## 4. 缓存层设计

### 4.1 元数据缓存 (`src/cache/metadata.rs`)

```rust
pub struct MetadataCache {
    /// 属性缓存 (inode -> attr)
    attr_cache: DashMap<u64, CachedAttr>,
    /// 目录列表缓存 (dir_inode -> children)
    dir_cache: DashMap<u64, CachedDirEntry>,
    /// 负缓存 (不存在的路径)
    negative_cache: DashMap<String, Instant>,
    /// 配置
    config: MetadataCacheConfig,
}

pub struct MetadataCacheConfig {
    pub attr_ttl: Duration,      // 属性缓存TTL (强一致性场景建议3-5秒)
    pub dir_ttl: Duration,       // 目录缓存TTL
    pub negative_ttl: Duration,  // 负缓存TTL
    pub max_entries: usize,      // 最大缓存条目
}
```

**强一致性保证机制**:
- 写操作后立即失效相关缓存
- 较短的TTL (默认3秒)
- 提供手动刷新接口
- 目录操作时刷新父目录缓存

### 4.2 数据块缓存 (`src/cache/data.rs`)

```rust
pub struct DataCache {
    /// 内存缓存 (热数据)
    memory_cache: DashMap<BlockKey, CachedBlock>,
    /// 磁盘缓存 (温数据)
    disk_cache: Option<DiskCache>,
    /// 配置
    block_size: u64,
    memory_limit: u64,
}

pub struct BlockKey {
    pub inode: u64,
    pub offset: u64,  // 块起始偏移 (对齐到block_size)
}
```

**缓存策略**:
- 两级LRU: 内存 -> 磁盘
- 写时失效 (write-invalidate)
- 后台清理线程
- 支持缓存预热

### 4.3 预读缓存 (`src/cache/readahead.rs`)

```rust
pub struct ReadaheadManager {
    /// 每个文件的读取状态
    file_states: DashMap<u64, ReadState>,
    /// 预读缓冲区
    prefetch_buffer: DashMap<PrefetchKey, PrefetchedData>,
    /// 配置
    config: ReadaheadConfig,
}

pub struct ReadaheadConfig {
    pub enable: bool,
    pub window_size: u64,        // 预读窗口 (默认16MB)
    pub concurrency: usize,      // 并发预读数 (默认4)
    pub seq_threshold: usize,    // 顺序读检测阈值
}
```

**预读算法**:
1. 检测顺序读取模式 (连续3次顺序读)
2. 动态调整预读窗口大小
3. 并发预取多个块
4. 避免重复预取

### 4.4 写缓冲 (`src/cache/write_buffer.rs`)

```rust
pub struct WriteBuffer {
    /// 每个文件的写缓冲区
    buffers: DashMap<u64, Arc<Mutex<FileWriteBuffer>>>,
    /// OBS客户端
    client: Arc<ObsClient>,
    /// 分片上传器
    multipart: MultipartUploader,
    /// 配置
    config: WriteBufferConfig,
}

pub struct FileWriteBuffer {
    pub inode: u64,
    pub path: String,
    pub data: BytesMut,
    pub dirty: bool,
    pub last_write: Instant,
    pub size: u64,
    pub multipart: Option<MultipartUploadState>,
}

pub struct WriteBufferConfig {
    pub buffer_size: usize,      // 单文件缓冲大小 (默认64MB)
    pub flush_interval: Duration, // 自动刷新间隔
    pub multipart_threshold: usize, // 分片上传阈值 (默认100MB)
    pub part_size: usize,        // 分片大小 (默认8MB)
}
```

**写入策略**:
- 小文件 (<100MB): 内存缓冲 + 单次PUT
- 大文件 (>=100MB): 分片上传
- 定时刷新 + 手动fsync
- 关闭时强制刷新

---

## 5. 存储层设计

### 5.1 OBS客户端封装 (`src/storage/obs.rs`)

```rust
pub struct ObsClient {
    operator: Operator,  // OpenDAL operator
    config: ObsConfig,
    metrics: Arc<Metrics>,
}

pub struct ObsConfig {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: String,
    pub max_connections: usize,
    pub request_timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
}
```

**主要操作**:
- `stat(path)` - 获取对象元数据
- `read(path)` - 读取整个对象
- `read_range(path, offset, size)` - 范围读取
- `write(path, data)` - 写入对象
- `delete(path)` - 删除对象
- `list_dir(prefix)` - 列出目录
- `create_dir(path)` - 创建目录标记
- `copy(from, to)` - 复制对象
- `rename(from, to)` - 重命名 (复制+删除)

### 5.2 分片上传 (`src/storage/multipart.rs`)

```rust
pub struct MultipartUploader {
    operator: Arc<Operator>,
    config: MultipartConfig,
    metrics: Arc<Metrics>,
}

pub struct MultipartConfig {
    pub part_size: usize,        // 分片大小 (8-64MB)
    pub concurrency: usize,      // 并发上传数 (默认5)
    pub max_parts: usize,        // 最大分片数 (10000)
}

pub struct MultipartUploadState {
    pub path: String,
    pub parts: Vec<PartInfo>,
    pub current_part: u32,
    pub buffer: Vec<u8>,
    pub total_bytes: u64,
}
```

### 5.3 重试逻辑 (`src/storage/retry.rs`)

```rust
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}
```

**重试策略**:
- 指数退避
- 可重试错误: 速率限制、连接超时、临时故障
- 不可重试错误: 认证失败、权限拒绝、对象不存在

---

## 6. 文件系统实现

### 6.1 ObsFs 结构 (`src/fs/obsfs.rs`)

```rust
pub struct ObsFs {
    inode_mgr: Arc<InodeManager>,
    metadata_cache: Arc<MetadataCache>,
    data_cache: Arc<DataCache>,
    readahead: Arc<ReadaheadManager>,
    write_buffer: Arc<WriteBuffer>,
    obs_client: Arc<ObsClient>,
    handle_mgr: Arc<HandleManager>,
    config: Arc<Config>,
    metrics: Arc<Metrics>,
}
```

### 6.2 实现的 FUSE 操作

| 操作 | 描述 | 缓存影响 |
|-----|------|---------|
| `init` | 初始化文件系统 | - |
| `destroy` | 销毁文件系统，刷新所有缓冲 | 清空所有缓存 |
| `lookup` | 查找目录项 | 读取/更新元数据缓存 |
| `getattr` | 获取文件属性 | 读取元数据缓存 |
| `setattr` | 设置文件属性 (chmod, truncate等) | 更新元数据缓存 |
| `open` | 打开文件 | 创建文件句柄 |
| `read` | 读取文件数据 | 读取数据缓存，触发预读 |
| `write` | 写入文件数据 | 写入缓冲，失效数据缓存 |
| `release` | 关闭文件 | 刷新写缓冲，关闭句柄 |
| `fsync` | 同步文件数据 | 强制刷新写缓冲 |
| `opendir` | 打开目录 | 创建目录句柄 |
| `readdir` | 读取目录内容 | 读取/更新目录缓存 |
| `releasedir` | 关闭目录 | 关闭目录句柄 |
| `create` | 创建文件 | 失效父目录缓存 |
| `mkdir` | 创建目录 | 失效父目录缓存 |
| `unlink` | 删除文件 | 失效所有相关缓存 |
| `rmdir` | 删除目录 | 失效所有相关缓存 |
| `rename` | 重命名/移动 | 失效源和目标目录缓存 |
| `statfs` | 获取文件系统统计 | - |
| `access` | 检查访问权限 | - |
| `flush` | 刷新文件 | 刷新写缓冲 |

---

## 7. 权限模型设计

### 7.1 可配置权限系统

```rust
pub enum PermissionMode {
    /// 固定权限模式: 所有文件使用相同的uid/gid/mode
    Fixed,
    /// 保留权限模式: 通过OBS对象元数据存储和恢复权限
    Preserved,
}

pub struct FixedPermission {
    pub uid: u32,
    pub gid: u32,
    pub file_mode: u32,  // 默认0644
    pub dir_mode: u32,   // 默认0755
}
```

### 7.2 权限存储 (保留模式)

在OBS对象的自定义元数据中存储权限信息:

| 元数据键 | 说明 | 示例 |
|---------|------|------|
| `x-obs-meta-uid` | 用户ID | 1000 |
| `x-obs-meta-gid` | 组ID | 1000 |
| `x-obs-meta-mode` | 权限位 | 33188 (0o100644) |
| `x-obs-meta-symlink-target` | 符号链接目标 | /path/to/target |

---

## 8. 强一致性实现

### 8.1 设计原则

1. **写后读一致性**: 写入成功后立即可读到最新数据
2. **关闭后打开一致性**: close后其他进程open能看到更新
3. **元数据一致性**: 文件属性变更立即生效

### 8.2 实现机制

```rust
// 写入时失效缓存
async fn write(&self, inode, offset, data) -> Result<usize> {
    // 1. 写入缓冲区
    self.write_buffer.write(inode, offset, data).await?;

    // 2. 失效数据缓存的相关块
    self.data_cache.invalidate_range(inode, offset, data.len());

    // 3. 更新元数据缓存 (mtime, size)
    self.metadata_cache.update_attr(inode, |attr| {
        attr.mtime = SystemTime::now();
        attr.size = attr.size.max(offset + data.len() as u64);
    });

    Ok(data.len())
}

// 关闭时强制刷新
async fn release(&self, inode, fh) -> Result<()> {
    // 强制刷新写缓冲到OBS
    self.write_buffer.release(inode).await?;

    // 失效元数据缓存确保一致性
    self.metadata_cache.invalidate_attr(inode);

    Ok(())
}

// fsync确保数据持久化
async fn fsync(&self, inode) -> Result<()> {
    // 同步刷新写缓冲
    self.write_buffer.sync_flush(inode).await?;
    Ok(())
}
```

---

## 9. 性能优化策略

### 9.1 读取优化

| 优化点 | 实现方式 | 预期效果 |
|-------|---------|---------|
| 并发读取 | Range GET + 多线程下载 | 大文件吞吐提升3-5x |
| 预读缓存 | 顺序读检测 + 异步预取 | 减少等待时间50%+ |
| 块缓存 | 两级LRU (内存+磁盘) | 重复读取延迟降低90%+ |
| 小文件优化 | 整文件缓存 | 小文件访问延迟<10ms |

### 9.2 写入优化

| 优化点 | 实现方式 | 预期效果 |
|-------|---------|---------|
| 写缓冲 | 内存buffer + 批量刷新 | 小写入延迟降低80%+ |
| 分片上传 | 并发多part上传 | 大文件上传速度提升5x |
| 写合并 | 小写入合并 | 减少API调用50%+ |
| 异步刷新 | 后台刷新线程 | 避免阻塞用户写入 |

### 9.3 元数据优化

| 优化点 | 实现方式 | 预期效果 |
|-------|---------|---------|
| 属性缓存 | DashMap + TTL | 重复查询延迟<1ms |
| 目录缓存 | LIST结果缓存 | ls命令响应<50ms |
| 负缓存 | 不存在路径缓存 | 减少无效API调用 |
| 批量LIST | 前缀批量查询 | 目录扫描速度提升10x |

### 9.4 FUSE层优化

```rust
// 挂载选项配置
let mount_options = MountOptions::default()
    .fs_name("obsfuse")
    .allow_root(true)
    .read_only(false);

// Session配置
let session = Session::new(mount_options)
    .max_write(4 * 1024 * 1024)   // 单次写入最大4MB
    .max_read(4 * 1024 * 1024);   // 单次读取最大4MB
```

---

## 10. 跨平台支持

### 10.1 平台差异处理

| 平台 | FUSE实现 | 注意事项 |
|-----|---------|---------|
| Linux | libfuse3/FUSE kernel module | 原生支持，性能最佳 |
| macOS | macFUSE/FUSE-T | 需要安装第三方FUSE实现 |
| 容器 | privileged模式或--device=/dev/fuse | 需要特殊权限配置 |

### 10.2 平台特定代码

```rust
// FileAttr 中的平台特定字段
pub struct FileAttr {
    // ... 通用字段 ...

    #[cfg(target_os = "macos")]
    pub crtime: SystemTime,  // 创建时间 (macOS特有)

    #[cfg(target_os = "macos")]
    pub flags: u32,          // 文件标志 (macOS特有)
}
```

---

## 11. 错误处理

### 11.1 错误类型映射

```rust
pub enum ObsFuseError {
    Io(io::Error),           // -> 原始errno
    Storage(opendal::Error), // -> 映射到FUSE错误
    Config(String),          // -> EINVAL
    InodeNotFound(u64),      // -> ENOENT
    PathNotFound(String),    // -> ENOENT
    HandleNotFound(u64),     // -> EBADF
    PermissionDenied(String),// -> EACCES
    FileExists(String),      // -> EEXIST
    NotADirectory(String),   // -> ENOTDIR
    IsADirectory(String),    // -> EISDIR
    DirectoryNotEmpty(String),// -> ENOTEMPTY
    InvalidArgument(String), // -> EINVAL
    NotSupported(String),    // -> ENOSYS
    // ...
}
```

### 11.2 OBS错误映射

| OBS错误 | FUSE错误码 |
|--------|-----------|
| NotFound | ENOENT |
| PermissionDenied | EACCES |
| AlreadyExists | EEXIST |
| RateLimited | EAGAIN |
| Unsupported | ENOSYS |
| 其他 | EIO |

---

## 12. 配置系统

### 12.1 配置来源优先级

1. 命令行参数 (最高)
2. 环境变量
3. 配置文件 (~/.obsfuse/config.toml)
4. 默认值 (最低)

### 12.2 配置结构

```toml
[obs]
endpoint = "obs.cn-north-1.myhuaweicloud.com"
bucket = "my-bucket"
region = "cn-north-1"
prefix = ""  # 可选的桶内前缀

[cache]
memory_limit = "512MB"
disk_limit = "10GB"
cache_dir = "/tmp/obsfuse"
block_size = "4MB"

[cache.metadata]
attr_ttl = "3s"
dir_ttl = "5s"
negative_ttl = "1s"
max_entries = 100000

[performance]
read_ahead = true
read_ahead_window = "16MB"
read_concurrency = 4
write_buffer_size = "64MB"
multipart_threshold = "100MB"
multipart_part_size = "8MB"
multipart_concurrency = 5
flush_interval = "30s"

[fuse]
max_read = "4MB"
max_write = "4MB"
allow_root = false
allow_other = false
read_only = false

[permission]
mode = "fixed"  # "fixed" 或 "preserved"

[permission.fixed]
uid = 1000
gid = 1000
file_mode = "0644"
dir_mode = "0755"

[logging]
level = "info"
file = "/var/log/obsfuse.log"  # 可选
```

---

## 13. 监控与指标

### 13.1 收集的指标

```rust
pub struct Metrics {
    // 读操作
    pub read_ops: AtomicU64,
    pub read_bytes: AtomicU64,
    pub read_cache_hits: AtomicU64,
    pub read_cache_misses: AtomicU64,

    // 写操作
    pub write_ops: AtomicU64,
    pub write_bytes: AtomicU64,
    pub write_buffer_flushes: AtomicU64,

    // 元数据操作
    pub lookup_ops: AtomicU64,
    pub getattr_ops: AtomicU64,
    pub readdir_ops: AtomicU64,
    pub metadata_cache_hits: AtomicU64,
    pub metadata_cache_misses: AtomicU64,

    // OBS操作
    pub obs_get_ops: AtomicU64,
    pub obs_put_ops: AtomicU64,
    pub obs_list_ops: AtomicU64,
    pub obs_delete_ops: AtomicU64,
    pub obs_errors: AtomicU64,
}
```

### 13.2 缓存命中率计算

```rust
// 数据缓存命中率
pub fn read_cache_hit_rate(&self) -> f64 {
    let hits = self.read_cache_hits.load(Ordering::Relaxed);
    let misses = self.read_cache_misses.load(Ordering::Relaxed);
    let total = hits + misses;
    if total == 0 { 0.0 } else { hits as f64 / total as f64 }
}

// 元数据缓存命中率
pub fn metadata_cache_hit_rate(&self) -> f64 {
    let hits = self.metadata_cache_hits.load(Ordering::Relaxed);
    let misses = self.metadata_cache_misses.load(Ordering::Relaxed);
    let total = hits + misses;
    if total == 0 { 0.0 } else { hits as f64 / total as f64 }
}
```

---

## 14. 关键风险与缓解

| 风险 | 缓解措施 |
|-----|---------|
| OBS API延迟高 | 多层缓存 + 预读 + 连接池 |
| 网络不稳定 | 重试机制 + 断点续传 |
| 内存溢出 | LRU淘汰 + 缓存大小限制 |
| 数据一致性 | 写后失效 + 短TTL |
| 并发冲突 | 文件句柄隔离 + 乐观锁 |

---

## 15. 测试策略

### 15.1 单元测试

- Inode管理: 创建、查找、删除、重命名
- 缓存模块: 存取、过期、淘汰
- 句柄管理: 打开、关闭、状态跟踪
- 路径操作: 解析、拼接、规范化

### 15.2 集成测试

- 文件操作: 创建、读、写、删除
- 目录操作: 创建、列出、删除
- 属性操作: 获取、设置、chmod
- 缓存一致性: 写后读、多进程访问

### 15.3 性能测试

- 顺序读写吞吐量
- 随机读写IOPS
- 元数据操作延迟
- 缓存命中率

---

## 附录 A: 依赖清单

```toml
[dependencies]
fuse3 = { version = "0.8", features = ["tokio-runtime", "unprivileged"] }
tokio = { version = "1", features = ["full"] }
opendal = { version = "0.55", features = ["services-obs"] }
dashmap = "6"
parking_lot = "0.12"
lru = "0.12"
moka = { version = "0.12", features = ["future"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
clap = { version = "4", features = ["derive", "env"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
bytes = "1"
bytesize = { version = "1", features = ["serde"] }
anyhow = "1"
thiserror = "1"
async-trait = "0.1"
futures = "0.3"
libc = "0.2"
nix = { version = "0.29", features = ["fs", "user"] }
chrono = { version = "0.4", features = ["serde"] }
humantime = "2"
dirs = "5"
```
