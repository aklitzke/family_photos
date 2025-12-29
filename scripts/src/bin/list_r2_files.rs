use scripts::list_r2_bucket;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (bucket_name, files) = list_r2_bucket("google_drive_pics").await?;

    println!("📦 Listing files in R2 bucket: {}", bucket_name);

    for file in &files {
        println!("  📄 {} ({} bytes)", file.key, file.size);
    }

    println!("\n✅ Total files: {}", files.len());

    Ok(())
}
