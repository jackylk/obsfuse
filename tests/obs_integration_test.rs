//! Integration test for OBS operations
//! Run with: cargo test --test obs_integration_test -- --nocapture
//!
//! All tests use the isolated folder: obsfuse-test/

use opendal::services::Obs;
use opendal::Operator;
use std::env;

/// Test folder prefix - all tests will be isolated under this folder
const TEST_PREFIX: &str = "obsfuse-test";

async fn create_operator() -> Operator {
    let builder = Obs::default()
        .endpoint("https://obs.cn-north-4.myhuaweicloud.com")
        .bucket("obs-fs-test-jska")
        .access_key_id(&env::var("OBS_ACCESS_KEY").expect("OBS_ACCESS_KEY not set"))
        .secret_access_key(&env::var("OBS_SECRET_KEY").expect("OBS_SECRET_KEY not set"));

    Operator::new(builder).unwrap().finish()
}

fn test_path(path: &str) -> String {
    format!("{}/{}", TEST_PREFIX, path)
}

#[tokio::test]
async fn test_00_cleanup_old_test_files() {
    let op = create_operator().await;

    println!("\n=== Cleaning Up Old Test Files ===\n");

    // Clean up old test files from previous runs (in root)
    let old_paths = vec![
        "test_files/hello.txt",
        "test_files/data.bin",
        "test_dirs/level1/level2/nested_file.txt",
        "test_dirs/level1/file_at_level1.txt",
        "test_dirs/root_level_file.txt",
    ];

    for path in old_paths {
        if op.exists(path).await.unwrap_or(false) {
            println!("   Deleting old file: {}", path);
            let _ = op.delete(path).await;
        }
    }

    println!("\n✅ Old test files cleaned up!\n");
}

#[tokio::test]
async fn test_01_obs_connection() {
    let op = create_operator().await;

    println!("\n=== Testing OBS Connection ===\n");

    // Test: List root directory
    println!("1. Listing root directory...");
    let entries = op.list("/").await.expect("Failed to list root");
    println!("   Found {} entries in root", entries.len());
    for entry in &entries {
        println!("   - {}", entry.path());
    }

    println!("\n✅ OBS connection successful!\n");
}

#[tokio::test]
async fn test_02_file_operations() {
    let op = create_operator().await;

    println!("\n=== Testing File Operations (in {}) ===\n", TEST_PREFIX);

    // Test 1: Create a simple text file
    let hello_path = test_path("files/hello.txt");
    println!("1. Creating test file: {}", hello_path);
    op.write(&hello_path, "Hello from OBS FUSE!\n")
        .await
        .expect("Failed to write file");
    println!("   ✅ File created successfully");

    // Test 2: Read the file back
    println!("\n2. Reading test file...");
    let content = op.read(&hello_path).await.expect("Failed to read file");
    let content_vec = content.to_vec();
    let text = String::from_utf8_lossy(&content_vec);
    println!("   Content: {}", text.trim());
    assert_eq!(text.trim(), "Hello from OBS FUSE!");
    println!("   ✅ File read successfully");

    // Test 3: Create a larger file
    let data_path = test_path("files/data.bin");
    println!("\n3. Creating larger file: {} (1MB)", data_path);
    let large_data: Vec<u8> = (0..1024*1024).map(|i| (i % 256) as u8).collect();
    op.write(&data_path, large_data.clone())
        .await
        .expect("Failed to write large file");
    println!("   ✅ Large file created successfully");

    // Test 4: Verify large file
    println!("\n4. Verifying large file...");
    let read_data = op.read(&data_path).await.expect("Failed to read large file");
    assert_eq!(read_data.len(), 1024*1024);
    assert_eq!(read_data.to_vec(), large_data);
    println!("   ✅ Large file verified (1MB)");

    // Test 5: Get file metadata
    println!("\n5. Getting file metadata...");
    let meta = op.stat(&hello_path).await.expect("Failed to stat file");
    println!("   Size: {} bytes", meta.content_length());
    println!("   Is file: {}", !meta.is_dir());
    println!("   ✅ Metadata retrieved successfully");

    // Test 6: Create files with different content types
    let json_path = test_path("files/config.json");
    println!("\n6. Creating JSON file: {}", json_path);
    op.write(&json_path, r#"{"name": "obsfuse", "version": "0.1.0"}"#)
        .await
        .expect("Failed to write JSON file");
    println!("   ✅ JSON file created");

    println!("\n✅ All file operations passed!\n");
}

#[tokio::test]
async fn test_03_directory_operations() {
    let op = create_operator().await;

    println!("\n=== Testing Directory Operations (in {}) ===\n", TEST_PREFIX);

    // Test 1: Create nested directory structure by creating files in it
    println!("1. Creating directory structure...");

    let nested_file = test_path("dirs/level1/level2/level3/deep_file.txt");
    op.write(&nested_file, "Deep nested content")
        .await
        .expect("Failed to create nested file");
    println!("   Created: {}", nested_file);

    let level1_file = test_path("dirs/level1/file_at_level1.txt");
    op.write(&level1_file, "Level 1 content")
        .await
        .expect("Failed to create level1 file");
    println!("   Created: {}", level1_file);

    let root_file = test_path("dirs/root_level_file.txt");
    op.write(&root_file, "Root level content")
        .await
        .expect("Failed to create root level file");
    println!("   Created: {}", root_file);
    println!("   ✅ Directory structure created");

    // Test 2: List directory contents
    let dirs_path = test_path("dirs/");
    println!("\n2. Listing {}...", dirs_path);
    let entries = op.list(&dirs_path).await.expect("Failed to list directory");
    println!("   Found {} entries:", entries.len());
    for entry in &entries {
        println!("   - {} (dir: {})", entry.path(), entry.metadata().is_dir());
    }

    // Test 3: List nested directory
    let level1_path = test_path("dirs/level1/");
    println!("\n3. Listing {}...", level1_path);
    let entries = op.list(&level1_path).await.expect("Failed to list nested directory");
    println!("   Found {} entries:", entries.len());
    for entry in &entries {
        println!("   - {}", entry.path());
    }

    // Test 4: Check if directory exists
    println!("\n4. Checking directory existence...");
    let exists = op.exists(&level1_path).await.expect("Failed to check existence");
    println!("   {} exists: {}", level1_path, exists);

    println!("\n✅ All directory operations passed!\n");
}

#[tokio::test]
async fn test_04_rename_and_copy() {
    let op = create_operator().await;

    println!("\n=== Testing Copy and Delete Operations (in {}) ===\n", TEST_PREFIX);

    // Test 1: Create a file to copy
    let original_path = test_path("copy_test/original.txt");
    println!("1. Creating file: {}", original_path);
    op.write(&original_path, "Original content for copy test")
        .await
        .expect("Failed to create file");
    println!("   ✅ File created");

    // Test 2: Copy file to new location
    let copied_path = test_path("copy_test/copied.txt");
    println!("\n2. Copying to: {}", copied_path);
    op.copy(&original_path, &copied_path)
        .await
        .expect("Failed to copy file");
    println!("   ✅ File copied");

    // Test 3: Verify both files exist
    println!("\n3. Verifying both files exist...");
    let original_exists = op.exists(&original_path).await.expect("Failed to check");
    let copied_exists = op.exists(&copied_path).await.expect("Failed to check");
    println!("   Original exists: {}", original_exists);
    println!("   Copied exists: {}", copied_exists);
    assert!(original_exists && copied_exists);
    println!("   ✅ Both files exist");

    // Test 4: Verify copied content
    println!("\n4. Verifying copied content...");
    let content = op.read(&copied_path).await.expect("Failed to read copied file");
    let content_vec = content.to_vec();
    let text = String::from_utf8_lossy(&content_vec);
    assert_eq!(text.as_ref(), "Original content for copy test");
    println!("   Content verified: {}", text);
    println!("   ✅ Copy content matches");

    // Test 5: Simulate rename by copy + delete
    let renamed_path = test_path("copy_test/renamed.txt");
    println!("\n5. Simulating rename (copy + delete)...");
    op.copy(&original_path, &renamed_path).await.expect("Failed to copy");
    op.delete(&original_path).await.expect("Failed to delete");
    let original_gone = !op.exists(&original_path).await.unwrap_or(true);
    let renamed_exists = op.exists(&renamed_path).await.expect("Failed to check");
    println!("   Original deleted: {}", original_gone);
    println!("   Renamed exists: {}", renamed_exists);
    assert!(original_gone && renamed_exists);
    println!("   ✅ Rename simulation successful");

    println!("\n✅ All copy/delete operations passed!\n");
}

#[tokio::test]
async fn test_05_list_test_folder() {
    let op = create_operator().await;

    println!("\n=== Listing All Files in Test Folder ({}) ===\n", TEST_PREFIX);

    let test_folder = format!("{}/", TEST_PREFIX);
    let entries = op.list_with(&test_folder).recursive(true).await.expect("Failed to list all");

    println!("Total entries in test folder: {}\n", entries.len());

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in &entries {
        let meta = entry.metadata();
        if meta.is_dir() {
            dirs.push(entry.path().to_string());
        } else {
            files.push((entry.path().to_string(), meta.content_length()));
        }
    }

    println!("📁 Directories ({}):", dirs.len());
    for dir in &dirs {
        println!("   {}", dir);
    }

    println!("\n📄 Files ({}):", files.len());
    for (file, size) in &files {
        println!("   {} ({} bytes)", file, size);
    }

    println!("\n✅ Listing complete!\n");
}

#[tokio::test]
async fn test_99_show_bucket_structure() {
    let op = create_operator().await;

    println!("\n=== Final Bucket Structure ===\n");

    let entries = op.list_with("/").recursive(true).await.expect("Failed to list all");

    println!("Total entries in bucket: {}\n", entries.len());

    for entry in &entries {
        let meta = entry.metadata();
        let path = entry.path();

        // Visual indicator for test folder vs other data
        let prefix = if path.starts_with(TEST_PREFIX) {
            "🧪"  // Test data
        } else {
            "📦"  // Other user data
        };

        if meta.is_dir() {
            println!("{} 📁 {}", prefix, path);
        } else {
            println!("{} 📄 {} ({} bytes)", prefix, path, meta.content_length());
        }
    }

    println!("\n🧪 = Test data (obsfuse-test/)");
    println!("📦 = Other user data");
    println!("\n✅ Bucket structure displayed!\n");
}
