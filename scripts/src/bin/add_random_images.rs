use common::ImageMetadata;
use rand::seq::SliceRandom;
use scripts::{list_r2_bucket, read_history, write_history};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Read existing history
    let mut history = read_history()?;
    println!("📚 Current history has {} images", history.images.len());

    // Find the highest ID
    let max_id = history
        .images
        .iter()
        .filter_map(|img| img.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    println!("📊 Highest ID: {}", max_id);

    // List all files in R2 bucket
    println!("🔍 Fetching files from R2 bucket...");
    let (bucket_name, files) = list_r2_bucket("google_drive_pics").await?;
    println!("📦 Found {} files in bucket: {}", files.len(), bucket_name);

    // Get existing keys to avoid duplicates
    let existing_keys: std::collections::HashSet<_> =
        history.images.iter().map(|img| &img.key).collect();

    // Filter out files that already exist
    let new_files: Vec<_> = files
        .iter()
        .filter(|f| !existing_keys.contains(&f.key))
        .collect();

    println!("🆕 Found {} new files", new_files.len());

    if new_files.is_empty() {
        println!("✅ No new files to add!");
        return Ok(());
    }

    // Pick up to 100 random files
    let count = new_files.len().min(10);
    let mut rng = rand::thread_rng();
    let selected = new_files.choose_multiple(&mut rng, count);

    println!("🎲 Randomly selected {} files", count);

    // Add them to history
    let mut next_id = max_id + 1;
    for file in selected {
        // Extract a name from the file path
        let name = file
            .key
            .split('/')
            .last()
            .unwrap_or(&file.key)
            .trim_end_matches(".jpg")
            .trim_end_matches(".jpeg")
            .trim_end_matches(".png")
            .to_string();

        history.images.push(ImageMetadata {
            id: next_id.to_string(),
            key: file.key.clone(),
            name,
        });

        next_id += 1;
    }

    // Write back to history.toml
    write_history(&history)?;

    println!("✅ Added {} new images to history.toml", count);
    println!("📚 Total images: {}", history.images.len());

    Ok(())
}
