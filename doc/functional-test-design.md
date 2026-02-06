# OBS 文件系统功能测试设计文档

> 本文档定义 OBS 文件系统的功能测试规范，适用于 obsfuse (Rust) 和 pyobs (Python) 项目。

## 1. 概述

### 1.1 测试目标

验证 OBS 文件系统实现的功能正确性，确保：
- 所有文件系统操作符合 POSIX 语义
- 数据完整性和一致性
- 错误处理正确性
- 边界条件处理

### 1.2 测试环境要求

| 要求 | 说明 |
|------|------|
| OBS 凭证 | 需要 `OBS_ACCESS_KEY` 和 `OBS_SECRET_KEY` |
| 测试桶 | 独立的测试桶或测试前缀目录 |
| 网络 | 稳定的网络连接到华为云 OBS |
| 清理 | 测试后清理所有测试文件 |

### 1.3 测试前缀约定

所有测试文件使用统一前缀，便于清理：
```
{project}-functest-{timestamp}/
```

示例：
- `obsfuse-functest-20240206/`
- `pyobs-functest-20240206/`

---

## 2. 测试分类

### 2.1 文件操作测试 (File Operations)

#### 2.1.1 创建文件 (Create)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| F-C-01 | create_empty_file | 创建空文件 | 文件存在，大小为 0 |
| F-C-02 | create_with_content | 创建带内容的文件 | 文件存在，内容正确 |
| F-C-03 | create_in_subdir | 在子目录中创建文件 | 文件存在于正确路径 |
| F-C-04 | create_special_chars | 文件名含特殊字符（空格、中文、emoji） | 正确创建和访问 |
| F-C-05 | create_long_name | 文件名接近最大长度（255字符） | 正确创建 |
| F-C-06 | create_deep_path | 深层嵌套路径（20层目录） | 正确创建 |
| F-C-07 | create_existing_file | 创建已存在的文件 | 覆盖或返回错误（取决于实现） |

#### 2.1.2 读取文件 (Read)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| F-R-01 | read_full_file | 读取完整文件 | 数据与写入一致 |
| F-R-02 | read_partial_start | 读取文件开头部分 | 正确的部分数据 |
| F-R-03 | read_partial_middle | 读取文件中间部分 | 正确的部分数据 |
| F-R-04 | read_partial_end | 读取文件结尾部分 | 正确的部分数据 |
| F-R-05 | read_beyond_eof | 读取超出文件末尾 | 返回空或 EOF |
| F-R-06 | read_empty_file | 读取空文件 | 返回空数据 |
| F-R-07 | read_large_file | 读取大文件（100MB） | 数据完整一致 |
| F-R-08 | read_nonexistent | 读取不存在的文件 | 返回 ENOENT 错误 |

#### 2.1.3 写入文件 (Write)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| F-W-01 | write_new_file | 写入新文件 | 文件创建，内容正确 |
| F-W-02 | write_overwrite | 覆盖已有文件 | 内容被替换 |
| F-W-03 | write_append | 追加写入 | 原内容保留，新内容追加 |
| F-W-04 | write_partial | 部分覆盖写入 | 只有指定部分被修改 |
| F-W-05 | write_large_file | 写入大文件（100MB） | 完整写入，可读取验证 |
| F-W-06 | write_binary_data | 写入二进制数据 | 数据不被修改 |
| F-W-07 | write_zero_bytes | 写入零字节 | 文件大小不变或截断 |

#### 2.1.4 删除文件 (Delete)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| F-D-01 | delete_file | 删除存在的文件 | 文件不再存在 |
| F-D-02 | delete_nonexistent | 删除不存在的文件 | 返回错误或静默成功 |
| F-D-03 | delete_readonly | 删除只读文件 | 取决于权限实现 |

#### 2.1.5 文件元数据 (Metadata)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| F-M-01 | stat_file | 获取文件状态 | 返回正确的 size, mtime |
| F-M-02 | stat_after_write | 写入后获取状态 | size 和 mtime 已更新 |
| F-M-03 | exists_check | 检查文件是否存在 | 返回正确的布尔值 |

---

### 2.2 目录操作测试 (Directory Operations)

#### 2.2.1 创建目录 (Mkdir)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| D-C-01 | mkdir_simple | 创建简单目录 | 目录存在 |
| D-C-02 | mkdir_nested | 创建嵌套目录 | 所有层级目录存在 |
| D-C-03 | mkdir_existing | 创建已存在的目录 | 返回错误或静默成功 |
| D-C-04 | mkdir_special_chars | 目录名含特殊字符 | 正确创建 |

#### 2.2.2 列出目录 (List/Readdir)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| D-L-01 | list_empty_dir | 列出空目录 | 返回空列表或只有 . 和 .. |
| D-L-02 | list_with_files | 列出含文件的目录 | 返回所有文件 |
| D-L-03 | list_with_subdirs | 列出含子目录的目录 | 返回所有子目录 |
| D-L-04 | list_mixed | 列出混合内容目录 | 文件和目录都返回 |
| D-L-05 | list_large_dir | 列出大目录（1000+条目） | 返回所有条目 |
| D-L-06 | list_nonexistent | 列出不存在的目录 | 返回 ENOENT 错误 |

#### 2.2.3 删除目录 (Rmdir)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| D-D-01 | rmdir_empty | 删除空目录 | 目录不再存在 |
| D-D-02 | rmdir_nonempty | 删除非空目录 | 返回 ENOTEMPTY 错误 |
| D-D-03 | rmdir_nonexistent | 删除不存在的目录 | 返回错误 |

---

### 2.3 重命名/移动操作 (Rename/Move)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| R-01 | rename_file_same_dir | 同目录重命名文件 | 旧名不存在，新名存在 |
| R-02 | rename_file_cross_dir | 跨目录移动文件 | 文件移动到新目录 |
| R-03 | rename_to_existing | 重命名到已存在的文件 | 覆盖目标文件 |
| R-04 | rename_dir | 重命名目录 | 目录及其内容移动 |
| R-05 | rename_nonexistent | 重命名不存在的文件 | 返回 ENOENT 错误 |

---

### 2.4 复制操作 (Copy)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| C-01 | copy_file | 复制文件 | 源和目标都存在，内容一致 |
| C-02 | copy_large_file | 复制大文件（100MB） | 内容完整一致 |
| C-03 | copy_to_existing | 复制到已存在的文件 | 目标被覆盖 |
| C-04 | copy_nonexistent | 复制不存在的文件 | 返回错误 |

---

### 2.5 一致性测试 (Consistency)

#### 2.5.1 写后读一致性 (Read-After-Write)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| CS-01 | raw_single | 单线程写后立即读 | 数据 100% 一致 |
| CS-02 | raw_concurrent | 多线程各自写后读 | 每个线程数据一致 |
| CS-03 | overwrite_visible | 覆盖写后立即读 | 读到新数据 |

#### 2.5.2 元数据一致性

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| CS-04 | size_after_write | 写入后文件大小正确 | size == 写入字节数 |
| CS-05 | mtime_updated | 写入后 mtime 更新 | mtime > 写入前 |
| CS-06 | create_visible | 创建后立即可见 | exists() == true |
| CS-07 | delete_invisible | 删除后立即不可见 | exists() == false |

#### 2.5.3 并发一致性

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| CS-08 | concurrent_create | 多线程创建不同文件 | 所有文件都存在 |
| CS-09 | concurrent_read_same | 多线程读同一文件 | 所有线程读到相同数据 |
| CS-10 | rename_atomic | 重命名原子性 | 任意时刻文件在且仅在一个路径 |

---

### 2.6 边界条件测试 (Boundary Conditions)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| BC-01 | empty_filename | 空文件名 | 返回错误 |
| BC-02 | max_filename | 最大长度文件名（255字符） | 正确处理 |
| BC-03 | max_path | 最大路径长度（4096字符） | 正确处理或返回错误 |
| BC-04 | zero_byte_write | 写入 0 字节 | 正确处理 |
| BC-05 | large_file_size | 5GB 文件（超过单次上传限制） | 使用分片上传 |
| BC-06 | special_chars | 文件名特殊字符：`空格 中文 émoji 🎉 \n \t` | 正确处理 |
| BC-07 | dot_files | 隐藏文件 `.hidden` | 正确处理 |
| BC-08 | double_slash | 路径含双斜杠 `a//b` | 正确规范化 |

---

### 2.7 错误处理测试 (Error Handling)

| 测试ID | 测试名称 | 描述 | 预期结果 |
|--------|----------|------|----------|
| E-01 | invalid_credentials | 无效凭证 | 返回认证错误 |
| E-02 | bucket_not_found | 桶不存在 | 返回 404 错误 |
| E-03 | permission_denied | 无权限操作 | 返回 403 错误 |
| E-04 | network_timeout | 网络超时 | 自动重试或返回错误 |
| E-05 | concurrent_delete | 并发删除同一文件 | 至少一个成功 |

---

## 3. 测试数据规范

### 3.1 文件大小分级

| 级别 | 大小 | 用途 |
|------|------|------|
| tiny | 1 字节 | 边界测试 |
| small | 1 KB | 基本功能测试 |
| medium | 1 MB | 常规测试 |
| large | 100 MB | 大文件测试 |
| huge | 5 GB | 分片上传测试（可选） |

### 3.2 测试数据生成

```
# 伪代码 - 生成可验证的测试数据
def generate_test_data(size: int) -> bytes:
    # 使用固定种子，确保可重复
    pattern = bytes([i % 256 for i in range(size)])
    return pattern

def verify_test_data(data: bytes, expected_size: int) -> bool:
    if len(data) != expected_size:
        return False
    expected = generate_test_data(expected_size)
    return data == expected
```

---

## 4. 测试执行

### 4.1 运行方式

```bash
# Rust (obsfuse)
cargo test --test functional           # 完整功能测试
cargo test --test functional -- F-C    # 只运行文件创建测试
cargo test --test functional -- CS     # 只运行一致性测试

# Python (pyobs)
pytest tests/functional/               # 完整功能测试
pytest tests/functional/ -k "F_C"      # 只运行文件创建测试
pytest tests/functional/ -k "CS"       # 只运行一致性测试
```

### 4.2 测试报告

生成统一格式的测试报告：

```json
{
    "project": "obsfuse",
    "timestamp": "2024-02-06T10:30:00Z",
    "total": 50,
    "passed": 48,
    "failed": 2,
    "skipped": 0,
    "results": [
        {
            "id": "F-C-01",
            "name": "create_empty_file",
            "status": "passed",
            "duration_ms": 150
        },
        ...
    ]
}
```

---

## 5. 跨项目复用

### 5.1 测试用例映射

| 测试ID | obsfuse (Rust) | pyobs (Python) |
|--------|---------------|----------------|
| F-C-01 | `test_create_empty_file` | `test_create_empty_file` |
| F-R-01 | `test_read_full_file` | `test_read_full_file` |
| ... | ... | ... |

### 5.2 共享测试数据

两个项目可以使用相同的测试数据文件：

```
tests/data/
├── small.bin      # 1KB 测试数据
├── medium.bin     # 1MB 测试数据
└── checksums.json # 校验和
```

---

## 6. 待确认事项

1. ~~**大文件测试**：5GB 文件测试是否必须？~~ ✅ 可选启用，默认跳过
2. ~~**网络故障模拟**：是否需要模拟网络故障场景？~~ ✅ 不需要
3. **权限测试**：是否需要测试不同 ACL 配置？（待定）
4. ~~**并发级别**：并发测试的线程数范围？~~ ✅ 1, 2, 4, 8
5. **超时设置**：各操作的超时时间？（待定）
6. ~~**跨项目一致**：如何确保 obsfuse 和 pyobs 测试一致？~~ ✅ JSON 规范文件

---

## 7. JSON 测试规范文件

两个项目共享 `tests/spec/functional-tests.json` 规范文件，确保测试用例一致。

详见：[tests/spec/functional-tests.json](../tests/spec/functional-tests.json)
