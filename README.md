# OBS FUSE

高性能 FUSE 文件系统，用于挂载华为云 OBS 对象存储，使其像本地文件系统一样使用。

## 特性

- **高性能**: 多级缓存、预读取、写缓冲
- **强一致性**: 写后读一致性保证
- **跨平台**: 支持 Linux 和 macOS
- **容器化**: 完整的 Docker 支持
- **灵活配置**: 可配置的权限模式和缓存设置

> 💡 **在 Python 程序中使用 OBS？** 请查看姊妹项目 [pyobs](https://github.com/pyobs/pyobs) - 基于 fsspec 的 Python SDK，可与 Pandas、Ray、Dask 等无缝集成。

## 目录

- [安装](#安装)
  - [Linux 安装](#linux-安装)
  - [macOS 安装](#macos-安装)
  - [从源码构建](#从源码构建)
- [使用方法](#使用方法)
  - [Linux 使用](#linux-使用)
  - [macOS 使用](#macos-使用)
  - [Docker 容器使用](#docker-容器使用)
- [配置](#配置)
- [性能优化](#性能优化)
- [故障排除](#故障排除)

---

## 安装

### Linux 安装

#### Ubuntu / Debian

```bash
# 1. 安装 FUSE3 依赖
sudo apt-get update
sudo apt-get install -y libfuse3-dev fuse3 pkg-config

# 2. 安装 Rust (如果未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 3. 克隆并构建
git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse
cargo build --release

# 4. 安装到系统
sudo cp target/release/obsfuse /usr/local/bin/

# 5. (可选) 允许普通用户使用 FUSE
sudo sed -i 's/#user_allow_other/user_allow_other/' /etc/fuse.conf
```

#### CentOS / RHEL / Fedora

```bash
# 1. 安装 FUSE3 依赖
sudo dnf install -y fuse3-devel fuse3 pkgconfig

# 2. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 3. 克隆并构建
git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse
cargo build --release

# 4. 安装
sudo cp target/release/obsfuse /usr/local/bin/
```

#### Arch Linux

```bash
# 1. 安装依赖
sudo pacman -S fuse3 rust

# 2. 克隆并构建
git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse
cargo build --release

# 3. 安装
sudo cp target/release/obsfuse /usr/local/bin/
```

### macOS 安装

macOS 需要安装第三方 FUSE 实现。推荐使用 **macFUSE** 或 **FUSE-T**。

#### 方法 1: 使用 macFUSE (推荐)

```bash
# 1. 安装 macFUSE
# 从 https://osxfuse.github.io/ 下载并安装
# 或使用 Homebrew:
brew install --cask macfuse

# 2. 重启电脑 (首次安装需要)
# 在系统偏好设置 > 安全性与隐私中允许系统扩展

# 3. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 4. 设置 PKG_CONFIG_PATH
export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH"

# 5. 克隆并构建
git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse
cargo build --release

# 6. 安装
sudo cp target/release/obsfuse /usr/local/bin/
```

#### 方法 2: 使用 FUSE-T

```bash
# 1. 安装 FUSE-T
brew install fuse-t

# 2. 安装 Rust 并构建 (同上)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse
cargo build --release
sudo cp target/release/obsfuse /usr/local/bin/
```

### 从源码构建

```bash
# 前置要求:
# - Rust 1.70+
# - FUSE3 开发库 (libfuse3-dev 或 macFUSE)
# - pkg-config

git clone https://github.com/obsfuse/obsfuse.git
cd obsfuse

# Debug 构建
cargo build

# Release 构建 (推荐生产环境)
cargo build --release

# 运行测试
cargo test

# 运行基准测试
cargo bench
```

---

## 使用方法

### Linux 使用

#### 基本挂载

```bash
# 1. 设置环境变量 (推荐方式)
export OBS_ACCESS_KEY="your_access_key"
export OBS_SECRET_KEY="your_secret_key"

# 2. 创建挂载点
sudo mkdir -p /mnt/obs

# 3. 挂载
obsfuse mount my-bucket /mnt/obs \
    --endpoint obs.cn-north-1.myhuaweicloud.com

# 4. 验证挂载
df -h /mnt/obs
ls -la /mnt/obs

# 5. 卸载
obsfuse unmount /mnt/obs
# 或
fusermount -u /mnt/obs
```

#### 开机自动挂载

创建 systemd 服务文件 `/etc/systemd/system/obsfuse.service`:

```ini
[Unit]
Description=OBS FUSE Filesystem
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Environment="OBS_ACCESS_KEY=your_access_key"
Environment="OBS_SECRET_KEY=your_secret_key"
ExecStart=/usr/local/bin/obsfuse mount my-bucket /mnt/obs \
    --endpoint obs.cn-north-1.myhuaweicloud.com \
    --foreground
ExecStop=/usr/local/bin/obsfuse unmount /mnt/obs
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
# 启用并启动服务
sudo systemctl daemon-reload
sudo systemctl enable obsfuse
sudo systemctl start obsfuse

# 查看状态
sudo systemctl status obsfuse
```

#### 允许其他用户访问

```bash
# 1. 编辑 /etc/fuse.conf，取消注释 user_allow_other
sudo sed -i 's/#user_allow_other/user_allow_other/' /etc/fuse.conf

# 2. 使用 --allow-other 选项挂载
obsfuse mount my-bucket /mnt/obs --allow-other
```

### macOS 使用

#### 基本挂载

```bash
# 1. 设置环境变量
export OBS_ACCESS_KEY="your_access_key"
export OBS_SECRET_KEY="your_secret_key"

# 2. 创建挂载点
mkdir -p ~/obs-mount

# 3. 挂载
obsfuse mount my-bucket ~/obs-mount \
    --endpoint obs.cn-north-1.myhuaweicloud.com

# 4. 在 Finder 中查看
open ~/obs-mount

# 5. 卸载
obsfuse unmount ~/obs-mount
# 或
umount ~/obs-mount
```

#### 使用 LaunchAgent 自动挂载

创建 `~/Library/LaunchAgents/com.obsfuse.mount.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.obsfuse.mount</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/obsfuse</string>
        <string>mount</string>
        <string>my-bucket</string>
        <string>/Users/username/obs-mount</string>
        <string>--endpoint</string>
        <string>obs.cn-north-1.myhuaweicloud.com</string>
        <string>--foreground</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>OBS_ACCESS_KEY</key>
        <string>your_access_key</string>
        <key>OBS_SECRET_KEY</key>
        <string>your_secret_key</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

```bash
# 加载服务
launchctl load ~/Library/LaunchAgents/com.obsfuse.mount.plist

# 卸载服务
launchctl unload ~/Library/LaunchAgents/com.obsfuse.mount.plist
```

### Docker 容器使用

#### 使用预构建镜像

```bash
# 构建镜像
docker build -t obsfuse:latest .

# 运行容器
docker run -d \
    --name obsfuse \
    --privileged \
    -e OBS_ACCESS_KEY="your_access_key" \
    -e OBS_SECRET_KEY="your_secret_key" \
    -e OBS_BUCKET="my-bucket" \
    -e OBS_ENDPOINT="obs.cn-north-1.myhuaweicloud.com" \
    -v /mnt/obs:/mnt/obs:rshared \
    obsfuse:latest
```

#### 使用 Docker Compose

创建 `.env` 文件:

```bash
OBS_ACCESS_KEY=your_access_key
OBS_SECRET_KEY=your_secret_key
OBS_BUCKET=my-bucket
OBS_ENDPOINT=obs.cn-north-1.myhuaweicloud.com
```

使用 `docker-compose.yml`:

```yaml
version: '3.8'

services:
  obsfuse:
    build: .
    image: obsfuse:latest
    container_name: obsfuse
    privileged: true
    volumes:
      - /mnt/obs:/mnt/obs:rshared
    environment:
      - OBS_ACCESS_KEY=${OBS_ACCESS_KEY}
      - OBS_SECRET_KEY=${OBS_SECRET_KEY}
      - OBS_BUCKET=${OBS_BUCKET}
      - OBS_ENDPOINT=${OBS_ENDPOINT}
      - LOG_LEVEL=info
    restart: unless-stopped
```

```bash
# 启动
docker-compose up -d

# 查看日志
docker-compose logs -f

# 停止
docker-compose down
```

#### Kubernetes 部署

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: obsfuse
spec:
  containers:
  - name: obsfuse
    image: obsfuse:latest
    securityContext:
      privileged: true
    env:
    - name: OBS_ACCESS_KEY
      valueFrom:
        secretKeyRef:
          name: obs-credentials
          key: access_key
    - name: OBS_SECRET_KEY
      valueFrom:
        secretKeyRef:
          name: obs-credentials
          key: secret_key
    - name: OBS_BUCKET
      value: "my-bucket"
    - name: OBS_ENDPOINT
      value: "obs.cn-north-1.myhuaweicloud.com"
    volumeMounts:
    - name: obs-mount
      mountPath: /mnt/obs
      mountPropagation: Bidirectional
  volumes:
  - name: obs-mount
    hostPath:
      path: /mnt/obs
      type: DirectoryOrCreate
```

#### 不使用 privileged 模式

如果不想使用 `privileged` 模式，可以使用更细粒度的权限:

```yaml
services:
  obsfuse:
    image: obsfuse:latest
    cap_add:
      - SYS_ADMIN
    devices:
      - /dev/fuse
    security_opt:
      - apparmor:unconfined
    volumes:
      - /mnt/obs:/mnt/obs:rshared
    # ... 其他配置
```

---

## 配置

### 命令行选项

```bash
obsfuse mount <bucket> <mountpoint> [OPTIONS]

OBS 连接选项:
    --endpoint <URL>           OBS 端点 URL
    --access-key <KEY>         访问密钥 (或设置 OBS_ACCESS_KEY 环境变量)
    --secret-key <KEY>         秘密密钥 (或设置 OBS_SECRET_KEY 环境变量)
    --region <REGION>          区域 [默认: cn-north-1]
    --prefix <PATH>            桶内前缀路径

缓存选项:
    --cache-dir <DIR>          缓存目录
    --memory-cache-size <SIZE> 内存缓存大小 [默认: 512MB]
    --disk-cache-size <SIZE>   磁盘缓存大小 [默认: 10GB]
    --metadata-ttl <SECS>      元数据 TTL 秒数 [默认: 3]

性能选项:
    --read-ahead               启用预读取
    --write-buffer-size <SIZE> 写缓冲大小 [默认: 64MB]

挂载选项:
    --allow-root               允许 root 访问
    --allow-other              允许其他用户访问
    --read-only                只读挂载
    -f, --foreground           前台运行

权限选项:
    --uid <UID>                固定用户 ID
    --gid <GID>                固定组 ID
    --file-mode <MODE>         文件权限模式 [默认: 0644]
    --dir-mode <MODE>          目录权限模式 [默认: 0755]

其他选项:
    -c, --config <FILE>        配置文件路径
    --log-level <LEVEL>        日志级别 [默认: info]
    --log-file <FILE>          日志文件路径
```

### 配置文件

创建 `~/.obsfuse/config.toml`:

```toml
[obs]
endpoint = "obs.cn-north-1.myhuaweicloud.com"
bucket = "my-bucket"
region = "cn-north-1"
# access_key 和 secret_key 建议使用环境变量

[cache]
memory_limit = "512MB"
disk_limit = "10GB"
cache_dir = "/tmp/obsfuse"
block_size = "4MB"

[cache.metadata]
attr_ttl = "3s"
dir_ttl = "5s"
negative_ttl = "1s"

[performance]
read_ahead = true
read_ahead_window = "16MB"
read_concurrency = 4
write_buffer_size = "64MB"
multipart_threshold = "100MB"
multipart_part_size = "8MB"
multipart_concurrency = 5

[fuse]
max_read = "4MB"
max_write = "4MB"
allow_root = false
allow_other = false

[permission]
mode = "fixed"

[permission.fixed]
uid = 1000
gid = 1000
file_mode = "0644"
dir_mode = "0755"

[logging]
level = "info"
```

### 华为云 OBS 区域端点

| 区域 | 端点 |
|-----|------|
| 华北-北京一 | obs.cn-north-1.myhuaweicloud.com |
| 华北-北京四 | obs.cn-north-4.myhuaweicloud.com |
| 华东-上海一 | obs.cn-east-3.myhuaweicloud.com |
| 华东-上海二 | obs.cn-east-2.myhuaweicloud.com |
| 华南-广州 | obs.cn-south-1.myhuaweicloud.com |
| 西南-贵阳一 | obs.cn-southwest-2.myhuaweicloud.com |

---

## 性能优化

### 读取优化
- **并发 Range GET**: 大文件使用多线程并发下载
- **顺序读检测**: 自动检测顺序读取模式并预取数据
- **两级缓存**: 内存 LRU + 磁盘 LRU 缓存

### 写入优化
- **写缓冲**: 小写入聚合后批量上传
- **分片上传**: 大文件 (>100MB) 自动使用分片并发上传
- **异步刷新**: 后台定期刷新，避免阻塞

### 元数据优化
- **属性缓存**: 减少重复 HEAD 请求
- **目录缓存**: 缓存 LIST 结果
- **负缓存**: 缓存不存在的路径，减少无效请求

### 推荐配置

#### 大文件场景 (视频、备份)

```toml
[cache]
memory_limit = "1GB"
block_size = "16MB"

[performance]
read_ahead_window = "64MB"
write_buffer_size = "128MB"
multipart_part_size = "32MB"
```

#### 小文件场景 (代码、配置)

```toml
[cache]
memory_limit = "256MB"
block_size = "1MB"

[cache.metadata]
attr_ttl = "5s"
dir_ttl = "10s"
max_entries = 500000
```

---

## 故障排除

### 常见问题

#### 1. 挂载失败: "fusermount: fuse device not found"

```bash
# Linux: 加载 fuse 模块
sudo modprobe fuse

# 检查 /dev/fuse 是否存在
ls -la /dev/fuse
```

#### 2. 权限拒绝

```bash
# 检查当前用户是否在 fuse 组
groups

# 添加用户到 fuse 组
sudo usermod -aG fuse $USER
# 重新登录生效
```

#### 3. macOS: "Operation not permitted"

1. 打开系统偏好设置 > 安全性与隐私 > 隐私
2. 允许 macFUSE 系统扩展
3. 重启电脑

#### 4. Docker: 挂载点在宿主机不可见

确保使用 `rshared` 挂载传播:

```bash
docker run -v /mnt/obs:/mnt/obs:rshared ...
```

#### 5. 性能问题

```bash
# 启用 debug 日志查看详情
obsfuse mount my-bucket /mnt/obs --log-level debug

# 检查缓存命中率
# 查看日志中的 cache hit/miss 统计
```

### 日志位置

- **前台运行**: 输出到 stderr
- **后台运行**: 使用 `--log-file` 指定
- **systemd**: `journalctl -u obsfuse`
- **Docker**: `docker logs obsfuse`

---

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                    应用程序                              │
└────────────────────────┬────────────────────────────────┘
                         │ POSIX API
┌────────────────────────▼────────────────────────────────┐
│                    FUSE 层                              │
│  • 异步请求处理                                          │
│  • 并发操作调度                                          │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                    缓存层                                │
│  ┌──────────────────┬───────────────────┐              │
│  │   元数据缓存      │   数据缓存         │              │
│  │  (DashMap+TTL)   │  (内存+磁盘LRU)   │              │
│  └──────────────────┴───────────────────┘              │
│  • 预读缓存 • 写缓冲                                     │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│              对象存储抽象层 (OpenDAL)                     │
│  • 连接池管理 • 分片上传 • 重试机制                       │
└────────────────────────┬────────────────────────────────┘
                         │ HTTPS
┌────────────────────────▼────────────────────────────────┐
│                    华为云 OBS                            │
└─────────────────────────────────────────────────────────┘
```

详细架构文档请参阅 [doc/architecture.md](doc/architecture.md)。

---

## 文档

- [架构设计文档](doc/architecture.md)
- [API 参考](doc/api-reference.md)

---

## 相关项目

- **[pyobs](https://github.com/pyobs/pyobs)** - 基于 fsspec 的 Python SDK，在 Python 程序中直接使用 OBS，支持 Pandas、Ray、Dask 等框架

---

## 许可证

MIT OR Apache-2.0
