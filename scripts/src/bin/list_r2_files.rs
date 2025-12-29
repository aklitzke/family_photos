use scripts::list_r2_bucket;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (_bucket_name, files) = list_r2_bucket("google_drive_pics").await?;

    for file in &files {
        println!("{}", file.key);
    }

    Ok(())
}
