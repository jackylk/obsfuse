//! Functional tests for OBS FUSE filesystem
//!
//! These tests verify the correctness of all filesystem operations
//! against real OBS storage.
//!
//! Run with:
//!   cargo test --test functional                    # All tests
//!   cargo test --test functional -- F_C             # File create tests
//!   cargo test --test functional -- --skip huge     # Skip large file tests
//!
//! Environment variables:
//!   OBS_ACCESS_KEY - OBS access key
//!   OBS_SECRET_KEY - OBS secret key
//!   FUNCTEST_PREFIX - Custom test prefix (default: obsfuse-functest)

use bytes::Bytes;
use opendal::services::Obs;
use opendal::Operator;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_PREFIX: &str = "obsfuse-functest";

fn get_test_prefix() -> String {
    let prefix = env::var("FUNCTEST_PREFIX").unwrap_or_else(|_| DEFAULT_PREFIX.to_string());
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}-{}", prefix, timestamp)
}

fn create_operator() -> Option<Operator> {
    let ak = env::var("OBS_ACCESS_KEY").ok()?;
    let sk = env::var("OBS_SECRET_KEY").ok()?;

    let builder = Obs::default()
        .endpoint("https://obs.cn-north-4.myhuaweicloud.com")
        .bucket("obs-fs-test-jska")
        .access_key_id(&ak)
        .secret_access_key(&sk);

    Operator::new(builder).ok().map(|op| op.finish())
}

fn test_path(prefix: &str, path: &str) -> String {
    format!("{}/{}", prefix, path)
}

// Generate deterministic test data for verification
fn generate_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

fn verify_test_data(data: &[u8], expected_size: usize) -> bool {
    if data.len() != expected_size {
        return false;
    }
    let expected = generate_test_data(expected_size);
    data == expected.as_slice()
}

// ============================================================================
// File Create Tests (F-C-*)
// ============================================================================

mod file_create {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn f_c_01_create_empty_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "empty.txt");

            // Create empty file
            op.write(&path, "").await.expect("Failed to create empty file");

            // Verify exists
            assert!(op.exists(&path).await.expect("Failed to check existence"));

            // Verify size is 0
            let meta = op.stat(&path).await.expect("Failed to stat");
            assert_eq!(meta.content_length(), 0);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_c_02_create_with_content() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "content.txt");
            let content = "Hello, OBS FUSE!";

            // Create file with content
            op.write(&path, content).await.expect("Failed to create file");

            // Read and verify
            let data = op.read(&path).await.expect("Failed to read");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_c_03_create_in_subdir() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "subdir/nested/file.txt");
            let content = "nested content";

            // Create file in nested path (OBS creates "directories" implicitly)
            op.write(&path, content).await.expect("Failed to create nested file");

            // Read and verify
            let data = op.read(&path).await.expect("Failed to read");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_c_04_create_special_chars() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "special 中文 file.txt");
            let content = "special chars";

            // Create file with special characters
            op.write(&path, content).await.expect("Failed to create special file");

            // Verify exists
            assert!(op.exists(&path).await.expect("Failed to check existence"));

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_c_06_create_deep_path() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let deep = "d1/d2/d3/d4/d5/d6/d7/d8/d9/d10/d11/d12/d13/d14/d15/d16/d17/d18/d19/d20";
            let path = test_path(&prefix, &format!("{}/file.txt", deep));
            let content = "deep";

            // Create file in deep path
            op.write(&path, content).await.expect("Failed to create deep file");

            // Read and verify
            let data = op.read(&path).await.expect("Failed to read");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }
}

// ============================================================================
// File Read Tests (F-R-*)
// ============================================================================

mod file_read {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn f_r_01_read_full_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "read_full.bin");
            let size = 65536; // 64KB
            let data = generate_test_data(size);

            // Write test data
            op.write(&path, data.clone()).await.expect("Failed to write");

            // Read and verify
            let read_data = op.read(&path).await.expect("Failed to read");
            assert!(verify_test_data(&read_data, size), "Data verification failed");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_r_02_read_partial_start() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "partial.txt");
            let content = "0123456789ABCDEF";

            op.write(&path, content).await.expect("Failed to write");

            // Read first 4 bytes
            let data = op.read_with(&path).range(0..4).await.expect("Failed to read range");
            assert_eq!(String::from_utf8_lossy(&data), "0123");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_r_03_read_partial_middle() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "partial_mid.txt");
            let content = "0123456789ABCDEF";

            op.write(&path, content).await.expect("Failed to write");

            // Read middle 4 bytes
            let data = op.read_with(&path).range(4..8).await.expect("Failed to read range");
            assert_eq!(String::from_utf8_lossy(&data), "4567");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_r_06_read_empty_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "empty_read.txt");

            op.write(&path, "").await.expect("Failed to write");

            let data = op.read(&path).await.expect("Failed to read");
            assert!(data.is_empty());

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_r_07_read_large_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "large.bin");
            let size = 10 * 1024 * 1024; // 10MB (reduced for faster tests)
            let data = generate_test_data(size);

            // Write
            op.write(&path, data).await.expect("Failed to write large file");

            // Read and verify
            let read_data = op.read(&path).await.expect("Failed to read large file");
            assert!(verify_test_data(&read_data, size), "Large file verification failed");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_r_08_read_nonexistent() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "nonexistent_file.txt");

            let result = op.read(&path).await;
            assert!(result.is_err(), "Reading nonexistent file should fail");
        });
    }
}

// ============================================================================
// File Write Tests (F-W-*)
// ============================================================================

mod file_write {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn f_w_01_write_new_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "new_write.txt");
            let content = "new content";

            op.write(&path, content).await.expect("Failed to write");

            let data = op.read(&path).await.expect("Failed to read");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_w_02_write_overwrite() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "overwrite.txt");

            // Write original
            op.write(&path, "old content").await.expect("Failed to write original");

            // Overwrite
            op.write(&path, "new content").await.expect("Failed to overwrite");

            // Verify new content
            let data = op.read(&path).await.expect("Failed to read");
            assert_eq!(String::from_utf8_lossy(&data), "new content");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_w_06_write_binary_data() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "binary.bin");
            let binary_data: Vec<u8> = vec![0, 1, 255, 128, 64, 0, 255];

            op.write(&path, binary_data.clone()).await.expect("Failed to write binary");

            let read_data = op.read(&path).await.expect("Failed to read binary");
            assert_eq!(read_data.to_vec(), binary_data);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }
}

// ============================================================================
// File Delete Tests (F-D-*)
// ============================================================================

mod file_delete {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn f_d_01_delete_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "to_delete.txt");

            // Create file
            op.write(&path, "delete me").await.expect("Failed to write");
            assert!(op.exists(&path).await.expect("Failed to check existence"));

            // Delete
            op.delete(&path).await.expect("Failed to delete");

            // Verify deleted
            assert!(!op.exists(&path).await.expect("Failed to check existence"));
        });
    }
}

// ============================================================================
// File Metadata Tests (F-M-*)
// ============================================================================

mod file_metadata {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn f_m_01_stat_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "stat_test.txt");
            let content = "0123456789"; // 10 bytes

            op.write(&path, content).await.expect("Failed to write");

            let meta = op.stat(&path).await.expect("Failed to stat");
            assert_eq!(meta.content_length(), 10);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn f_m_03_exists_check() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "exists_test.txt");

            // Should not exist initially
            assert!(!op.exists(&path).await.expect("Failed to check existence"));

            // Create
            op.write(&path, "exists").await.expect("Failed to write");

            // Should exist now
            assert!(op.exists(&path).await.expect("Failed to check existence"));

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }
}

// ============================================================================
// Directory Tests (D-*)
// ============================================================================

mod directory {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn d_c_01_mkdir_simple() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "newdir/");

            op.create_dir(&path).await.expect("Failed to create dir");
            assert!(op.exists(&path).await.expect("Failed to check existence"));

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn d_l_02_list_with_files() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let dir = test_path(&prefix, "list_test/");

            // Create files
            op.write(&format!("{}a.txt", dir), "a").await.expect("Failed to write a");
            op.write(&format!("{}b.txt", dir), "b").await.expect("Failed to write b");
            op.write(&format!("{}c.txt", dir), "c").await.expect("Failed to write c");

            // List
            let entries = op.list(&dir).await.expect("Failed to list");
            let names: Vec<_> = entries.iter().map(|e| e.name()).collect();

            assert!(names.contains(&"a.txt"));
            assert!(names.contains(&"b.txt"));
            assert!(names.contains(&"c.txt"));

            // Cleanup
            let _ = op.delete(&format!("{}a.txt", dir)).await;
            let _ = op.delete(&format!("{}b.txt", dir)).await;
            let _ = op.delete(&format!("{}c.txt", dir)).await;
        });
    }
}

// ============================================================================
// Rename/Copy Tests (R-*, C-*)
// ============================================================================

mod rename_copy {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn r_01_rename_file_same_dir() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let old_path = test_path(&prefix, "old_name.txt");
            let new_path = test_path(&prefix, "new_name.txt");
            let content = "rename test";

            // Create original
            op.write(&old_path, content).await.expect("Failed to write");

            // Rename (copy + delete in OBS)
            op.copy(&old_path, &new_path).await.expect("Failed to copy");
            op.delete(&old_path).await.expect("Failed to delete old");

            // Verify
            assert!(!op.exists(&old_path).await.expect("Failed to check old"));
            assert!(op.exists(&new_path).await.expect("Failed to check new"));

            let data = op.read(&new_path).await.expect("Failed to read new");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&new_path).await;
        });
    }

    #[test]
    fn c_01_copy_file() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let src = test_path(&prefix, "copy_src.txt");
            let dst = test_path(&prefix, "copy_dst.txt");
            let content = "copy test content";

            // Create source
            op.write(&src, content).await.expect("Failed to write source");

            // Copy
            op.copy(&src, &dst).await.expect("Failed to copy");

            // Both should exist
            assert!(op.exists(&src).await.expect("Failed to check source"));
            assert!(op.exists(&dst).await.expect("Failed to check dest"));

            // Content should match
            let data = op.read(&dst).await.expect("Failed to read dest");
            assert_eq!(String::from_utf8_lossy(&data), content);

            // Cleanup
            let _ = op.delete(&src).await;
            let _ = op.delete(&dst).await;
        });
    }
}

// ============================================================================
// Consistency Tests (CS-*)
// ============================================================================

mod consistency {
    use super::*;
    use tokio::runtime::Runtime;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn cs_01_raw_single() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            // Test read-after-write consistency 10 times
            for i in 0..10 {
                let path = test_path(&prefix, &format!("raw_single_{}.bin", i));
                let size = 1024;
                let data = generate_test_data(size);

                // Write
                op.write(&path, data).await.expect("Failed to write");

                // Immediate read
                let read_data = op.read(&path).await.expect("Failed to read");
                assert!(verify_test_data(&read_data, size), "Data mismatch at iteration {}", i);

                // Cleanup
                let _ = op.delete(&path).await;
            }
        });
    }

    #[test]
    fn cs_06_create_visible() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "visible_test.txt");

            // Create
            op.write(&path, "test").await.expect("Failed to write");

            // Should be immediately visible
            assert!(op.exists(&path).await.expect("Failed to check"),
                "File should be visible immediately after creation");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn cs_07_delete_invisible() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "invisible_test.txt");

            // Create
            op.write(&path, "test").await.expect("Failed to write");
            assert!(op.exists(&path).await.expect("Failed to check"));

            // Delete
            op.delete(&path).await.expect("Failed to delete");

            // Should be immediately invisible
            assert!(!op.exists(&path).await.expect("Failed to check"),
                "File should be invisible immediately after deletion");
        });
    }
}

// ============================================================================
// Boundary Condition Tests (BC-*)
// ============================================================================

mod boundary {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn bc_04_zero_byte_write() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, "zero_write.txt");

            // Write original content
            op.write(&path, "original").await.expect("Failed to write");

            // Overwrite with empty
            op.write(&path, "").await.expect("Failed to write empty");

            // Verify size is 0
            let meta = op.stat(&path).await.expect("Failed to stat");
            assert_eq!(meta.content_length(), 0);

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }

    #[test]
    fn bc_07_dot_files() {
        let Some(op) = create_operator() else {
            eprintln!("Skipping: OBS credentials not set");
            return;
        };
        let rt = Runtime::new().unwrap();
        let prefix = get_test_prefix();

        rt.block_on(async {
            let path = test_path(&prefix, ".hidden");

            op.write(&path, "hidden content").await.expect("Failed to write hidden");

            let data = op.read(&path).await.expect("Failed to read hidden");
            assert_eq!(String::from_utf8_lossy(&data), "hidden content");

            // Cleanup
            let _ = op.delete(&path).await;
        });
    }
}
