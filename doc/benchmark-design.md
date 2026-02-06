# OBS 文件系统性能测试设计文档

> 本文档定义 OBS 文件系统的性能基准测试方案，适用于 obsfuse (Rust) 和 pyobs (Python) 项目。

## 1. 概述

### 1.1 测试目标

| 目标 | 说明 |
|------|------|
| 单线程基准性能 | 测量各组件在无竞争状态下的纯性能 |
| 多线程并发性能 | 测量高并发访问下的吞吐量和扩展性 |
| OBS 集成性能 | 测量真实网络条件下的读写性能 |
| 缓存效率 | 测量缓存命中率对性能的影响 |

### 1.2 测试模式

| 模式 | 采样数 | 测量时间 | 预估耗时 | 用途 |
|------|--------|----------|----------|------|
| **quick** | 10 | 1秒 | ~2分钟 | 快速验证、CI/CD |
| **normal** | 50 | 5秒 | ~10分钟 | 日常开发 |
| **full** | 100 | 10秒 | ~30分钟 | 发布前完整测试 |

## 3. 测试分类

### 3.1 强一致性测试

文件系统操作必须保证强一致性，以下测试验证并发场景下的数据正确性：

#### 3.1.1 写后读一致性 (Read-After-Write)

| 测试项 | 说明 | 预期结果 |
|--------|------|----------|
| `consistency_raw` | 写入后立即读取，验证数据一致 | 100% 数据匹配 |
| `consistency_raw_concurrent` | 多线程并发写入不同文件，各自读取验证 | 100% 数据匹配 |
| `consistency_overwrite` | 覆盖写入后读取 | 读取到新数据 |

#### 3.1.2 元数据一致性

| 测试项 | 说明 | 预期结果 |
|--------|------|----------|
| `consistency_size_after_write` | 写入后文件大小正确 | size == 写入字节数 |
| `consistency_mtime_update` | 写入后 mtime 更新 | mtime > 写入前 mtime |
| `consistency_create_visible` | 创建文件后立即可见 | exists() == true |
| `consistency_delete_invisible` | 删除文件后立即不可见 | exists() == false |

#### 3.1.3 并发写入一致性

| 测试项 | 说明 | 预期结果 |
|--------|------|----------|
| `consistency_concurrent_append` | 多线程追加写入同一文件 | 数据完整无丢失 |
| `consistency_concurrent_overwrite` | 多线程覆盖写入同一文件 | 最终数据为某一线程写入的完整数据 |
| `consistency_rename_atomic` | 重命名操作原子性 | 任意时刻文件要么在旧路径，要么在新路径 |

#### 3.1.4 缓存一致性

| 测试项 | 说明 | 预期结果 |
|--------|------|----------|
| `consistency_cache_invalidate` | 写入后缓存失效 | 读取到最新数据 |
| `consistency_metadata_cache` | 元数据更新后缓存失效 | 获取到最新元数据 |
| `consistency_negative_cache` | 创建文件后负缓存失效 | 新文件立即可查找 |

### 3.2 本地组件测试（无网络依赖）

#### 3.2.1 Inode 管理性能

| 测试项 | 说明 | 预期指标 |
|--------|------|----------|
| `inode_create` | 创建新 inode | > 500K ops/sec |
| `inode_lookup` | 查找已存在的 inode | > 1M ops/sec |
| `inode_path_resolve` | inode 转路径 | > 1M ops/sec |
| `inode_concurrent_create` | 多线程并发创建 | 线性扩展 |

#### 3.2.2 元数据缓存性能

| 测试项 | 说明 | 预期指标 |
|--------|------|----------|
| `metadata_cache_put` | 写入属性缓存 | > 500K ops/sec |
| `metadata_cache_get_hit` | 缓存命中读取 | > 2M ops/sec |
| `metadata_cache_get_miss` | 缓存未命中 | > 1M ops/sec |
| `metadata_cache_concurrent` | 并发读写混合 | 线性扩展 |

#### 3.2.3 数据缓存性能

| 测试项 | 说明 | 预期指标 |
|--------|------|----------|
| `data_cache_put_64k` | 写入 64KB 数据块 | > 10 GB/s |
| `data_cache_get_hit` | 缓存命中读取 | > 20 GB/s |
| `data_cache_eviction` | LRU 驱逐性能 | < 1ms/evict |
| `data_cache_concurrent` | 并发读写 | 线性扩展 |

#### 3.2.4 预读取管理器性能

| 测试项 | 说明 | 预期指标 |
|--------|------|----------|
| `readahead_record` | 记录读取模式 | > 1M ops/sec |
| `readahead_detect_seq` | 顺序读检测 | > 500K ops/sec |
| `readahead_store_prefetch` | 存储预取数据 | > 5 GB/s |

#### 3.2.5 路径操作性能

| 测试项 | 说明 | 预期指标 |
|--------|------|----------|
| `path_join` | 路径拼接 | > 10M ops/sec |
| `path_parent` | 获取父路径 | > 10M ops/sec |
| `path_filename` | 获取文件名 | > 10M ops/sec |

### 3.3 OBS 集成测试（需要网络）

#### 3.3.1 单文件操作性能

| 测试项 | 文件大小 | 说明 |
|--------|----------|------|
| `obs_write_small` | 1KB | 小文件写入延迟 |
| `obs_write_medium` | 1MB | 中等文件写入吞吐 |
| `obs_write_large` | 100MB | 大文件写入吞吐 |
| `obs_read_small` | 1KB | 小文件读取延迟 |
| `obs_read_medium` | 1MB | 中等文件读取吞吐 |
| `obs_read_large` | 100MB | 大文件读取吞吐 |
| `obs_read_range` | 64KB range | 范围读取性能 |

#### 3.3.2 目录操作性能

| 测试项 | 说明 |
|--------|------|
| `obs_list_small_dir` | 列出 10 个文件的目录 |
| `obs_list_large_dir` | 列出 1000 个文件的目录 |
| `obs_stat` | 获取文件元数据 |
| `obs_exists` | 检查文件是否存在 |

#### 3.3.3 并发 OBS 操作

| 测试项 | 并发数 | 说明 |
|--------|--------|------|
| `obs_concurrent_read` | 1/4/8/16 | 并发读取吞吐量 |
| `obs_concurrent_write` | 1/4/8/16 | 并发写入吞吐量 |
| `obs_concurrent_mixed` | 8 | 混合读写负载 |

## 4. 测试配置

### 4.1 本地测试参数

```toml
[local_benchmark]
warmup_iterations = 100
measurement_iterations = 1000
sample_size = 100
```

### 4.2 OBS 集成测试参数

```toml
[obs_benchmark]
test_prefix = "obsfuse-bench"     # 测试文件前缀
cleanup_after = true              # 测试后清理
warmup_iterations = 5
measurement_iterations = 20
concurrent_levels = [1, 4, 8, 16, 32]
```

## 5. 并发测试设计

### 5.1 线程扩展性测试

测试不同线程数下的吞吐量变化：

```
线程数: 1 -> 2 -> 4 -> 8 -> 16 -> 32
```

预期结果：
- 理想情况：线性扩展（N 线程 = N 倍吞吐）
- 实际情况：随线程增加，扩展效率递减

### 5.2 竞争测试

| 场景 | 说明 |
|------|------|
| 读-读并发 | 多线程读取同一 inode |
| 写-写并发 | 多线程写入不同 inode |
| 读-写混合 | 读写比例 8:2 |
| 热点访问 | 80% 请求访问 20% 数据 |

## 6. 输出报告

### 6.1 报告格式

| 格式 | 文件 | 用途 |
|------|------|------|
| Markdown | `test-reports/benchmark-report.md` | 人类可读报告 |
| JSON | `test-reports/benchmark-data.json` | 机器可读数据 |
| HTML | `test-reports/criterion-report/` | 交互式图表 |

### 6.2 报告内容

```markdown
# 性能测试报告

## 测试环境
- OS: macOS/Linux
- CPU: ...
- Memory: ...
- 测试时间: ...

## 摘要
- 本地组件: X 项测试, 全部通过
- OBS 集成: Y 项测试, 全部通过

## 详细结果
### Inode 操作
| 测试项 | 平均耗时 | P50 | P99 | ops/sec |
|--------|----------|-----|-----|---------|
| ... | ... | ... | ... | ... |

### 并发扩展性
| 线程数 | 吞吐量 | 扩展效率 |
|--------|--------|----------|
| 1 | 100K | 100% |
| 4 | 380K | 95% |
| 8 | 720K | 90% |
```

## 7. 运行方式

### 7.1 Rust (obsfuse)

```bash
# 快速测试（约2分钟）
cargo bench --bench benchmark -- --quick

# 普通测试（约10分钟，默认）
cargo bench --bench benchmark

# 完整测试（约30分钟）
cargo bench --bench benchmark -- --full

# 包含 OBS 集成测试（需要凭证）
source .obs-credentials
cargo bench --bench benchmark --features obs-bench

# 只测试特定组件
cargo bench --bench benchmark -- inode        # Inode 操作
cargo bench --bench benchmark -- cache        # 缓存操作
cargo bench --bench benchmark -- concurrent   # 并发测试
cargo bench --bench benchmark -- obs          # OBS 集成

# 生成完整报告
./scripts/run-benchmark.sh              # 本地测试 + 报告
./scripts/run-benchmark.sh --with-obs   # 包含 OBS 测试
./scripts/run-benchmark.sh --full       # 完整测试 + 回归检测
```

### 7.2 Python (pyobs)

```bash
# 快速测试
pytest tests/benchmark/ --quick

# 普通测试
pytest tests/benchmark/

# 完整测试
pytest tests/benchmark/ --full

# 包含 OBS 集成测试
pytest tests/benchmark/ --with-obs

# 只测试特定组件
pytest tests/benchmark/ -k "inode"
pytest tests/benchmark/ -k "cache"
pytest tests/benchmark/ -k "concurrent"

# 生成报告
python scripts/run_benchmark.py --report
```

### 7.3 一致性测试（两个项目通用）

```bash
# Rust
cargo test --test consistency

# Python
pytest tests/consistency/
```

## 8. 目录结构

```
obsfuse/                           # 或 pyobs/
├── benches/                       # Rust benchmarks
│   ├── benchmark.rs               # 主入口
│   ├── local_bench.rs             # 本地组件测试
│   ├── concurrent_bench.rs        # 并发测试
│   ├── obs_bench.rs               # OBS 集成测试
│   └── consistency_tests.rs       # 一致性测试
├── tests/
│   ├── spec/
│   │   ├── functional-tests.json  # 功能测试规范（共享）
│   │   └── benchmark-spec.json    # 性能测试规范（共享）
│   ├── functional/                # 功能测试实现
│   └── benchmark/                 # Python benchmark（pyobs）
├── test-reports/
│   ├── benchmark-report.md        # Markdown 报告
│   ├── benchmark-data.json        # JSON 数据
│   ├── baseline.json              # 基准数据（用于回归检测）
│   ├── criterion-report/          # HTML 报告
│   └── history/                   # 历史数据
├── scripts/
│   ├── run-benchmark.sh           # 测试运行脚本
│   ├── generate-report.sh         # 报告生成脚本
│   └── check-regression.sh        # 回归检测脚本
└── doc/
    ├── benchmark-design.md        # 性能测试设计文档
    └── functional-test-design.md  # 功能测试设计文档
```

## 9. 跨项目共享

### 9.1 共享规范文件

两个项目使用相同的 JSON 规范文件：

| 文件 | 用途 |
|------|------|
| `tests/spec/functional-tests.json` | 功能测试用例定义 |
| `tests/spec/benchmark-spec.json` | 性能测试用例定义 |

### 9.2 报告格式统一

两个项目生成相同格式的报告，便于对比：

```json
{
    "project": "obsfuse",  // 或 "pyobs"
    "version": "0.1.0",
    "timestamp": "2024-02-06T10:30:00Z",
    "mode": "normal",
    "benchmarks": {
        "inode/create": {
            "mean_ns": 1234,
            "std_dev_ns": 56,
            "ops_per_sec": 810372
        }
    }
}
```

### 9.3 测试用例映射

| 测试ID | obsfuse (Rust) | pyobs (Python) |
|--------|---------------|----------------|
| inode/create | `benchmark_inode_create` | `test_benchmark_inode_create` |
| cache/get_hit | `benchmark_cache_get_hit` | `test_benchmark_cache_get_hit` |
| obs/write_1MB | `benchmark_obs_write_1mb` | `test_benchmark_obs_write_1mb` |

---

**待确认事项：**

1. ~~OBS 集成测试的大文件测试（100MB）是否可接受？~~ ✅ 保留
2. ~~并发测试的最大线程数 16 是否足够？~~ ✅ 增加到 32
3. ~~是否需要添加内存占用监控？~~ ✅ 需要
4. ~~是否需要历史数据对比（回归检测）？~~ ✅ 需要
5. ~~是否需要火焰图？~~ ✅ 需要

## 9. 额外功能

### 9.1 内存监控

在测试期间记录内存使用峰值，输出到报告中。

### 9.2 回归检测

与 `test-reports/baseline.json` 对比，检测性能回归：
- 性能下降 > 10%：警告
- 性能下降 > 20%：失败

### 9.3 火焰图

使用 `cargo-flamegraph` 生成火焰图：
```bash
cargo flamegraph --bench benchmark -o test-reports/flamegraph.svg
```
