//! Test OBS connection with provided credentials

use opendal::{Operator, services};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bucket = env::args().nth(1).unwrap_or_else(|| "obs-fs-test-jska".to_string());
    let access_key = env::var("OBS_ACCESS_KEY").expect("OBS_ACCESS_KEY not set");
    let secret_key = env::var("OBS_SECRET_KEY").expect("OBS_SECRET_KEY not set");
    let endpoint = env::var("OBS_ENDPOINT")
        .unwrap_or_else(|_| "https://obs.cn-north-1.myhuaweicloud.com".to_string());

    println!("Testing OBS connection:");
    println!("  Bucket: {}", bucket);
    println!("  Endpoint: {}", endpoint);
    println!("  Access Key: {}...", &access_key[..10.min(access_key.len())]);
    println!();

    // Build OBS operator
    let builder = services::Obs::default()
        .root("/")
        .bucket(&bucket)
        .endpoint(&endpoint)
        .access_key_id(&access_key)
        .secret_access_key(&secret_key);

    let op = Operator::new(builder)?.finish();

    println!("Attempting to list bucket contents...");
    match op.list("/").await {
        Ok(entries) => {
            println!("✓ Connection successful!");
            println!("\nFiles in bucket:");
            let count = entries.len();
            for (i, entry) in entries.iter().enumerate() {
                if i >= 10 {
                    println!("  ... (showing first 10 of {} entries)", count);
                    break;
                }
                let path = entry.path();
                let mode = entry.metadata().mode();
                println!("  - {} ({:?})", path, mode);
            }
            if count == 0 {
                println!("  (bucket is empty)");
            }
        }
        Err(e) => {
            eprintln!("✗ Connection failed:");
            eprintln!("  Error: {}", e);
            eprintln!("\nPossible causes:");
            eprintln!("  1. Invalid credentials");
            eprintln!("  2. No permission to access bucket '{}'", bucket);
            eprintln!("  3. Bucket doesn't exist or is in a different region");
            eprintln!("\nPlease check:");
            eprintln!("  - Credentials are correct in Huawei Cloud console");
            eprintln!("  - IAM user has 'obs:bucket:ListBucket' permission");
            eprintln!("  - Bucket name and region are correct");
            return Err(e.into());
        }
    }

    Ok(())
}
