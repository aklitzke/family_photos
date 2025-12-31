use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const THUMBNAIL_WIDTH: u32 = 300;

fn is_image_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "tiff" | "tif" | "bmp" | "gif" | "webp"
        )
    } else {
        false
    }
}

fn get_thumbnail_path(source_path: &Path, source_dir: &Path, dest_dir: &Path) -> PathBuf {
    let relative_path = source_path.strip_prefix(source_dir).unwrap();

    // Change extension to .jpg for thumbnail
    let mut thumbnail_relative = relative_path.to_path_buf();
    thumbnail_relative.set_extension("jpg");

    dest_dir.join(thumbnail_relative)
}

fn process_image(
    source_path: &Path,
    thumbnail_path: &Path,
) -> Result<(), Box<dyn Error>> {
    use image::ImageReader;
    use image::imageops::FilterType;

    // Check if thumbnail already exists
    if thumbnail_path.exists() {
        println!("   ⏭️  Thumbnail already exists, skipping");
        return Ok(());
    }

    // Get file size
    let file_size_mb = fs::metadata(source_path)?.len() as f64 / 1_048_576.0;
    println!("   Size: {:.2} MB", file_size_mb);

    // Generate thumbnail
    print!("   Generating thumbnail...");

    // Open image and remove limits (allows large images over 512MB default)
    let mut reader = ImageReader::open(source_path)?;
    reader.no_limits();

    // Decode image
    let img = reader.decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    // Calculate height maintaining aspect ratio
    let height = (THUMBNAIL_WIDTH as f32 * img.height() as f32 / img.width() as f32) as u32;

    // Resize image using Triangle filter (fast and good quality)
    let thumbnail = img.resize(THUMBNAIL_WIDTH, height, FilterType::Triangle);

    // Create parent directory if it doesn't exist
    if let Some(parent) = thumbnail_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Encode and save as JPEG
    thumbnail.save(thumbnail_path)?;

    let thumb_size = fs::metadata(thumbnail_path)?.len() as f64 / 1024.0;
    println!(" ✓ ({:.2} KB)", thumb_size);

    println!("   ✅ Complete!\n");

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <source_directory> <destination_directory>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} /Users/user/Pictures/Photos /Users/user/Pictures/Thumbnails", args[0]);
        eprintln!("\nThis script will:");
        eprintln!("  - Scan source directory recursively for images");
        eprintln!("  - Generate thumbnails for images that don't have them in destination");
        eprintln!("  - Preserve directory structure in destination");
        eprintln!("  - Skip images that already have thumbnails");
        std::process::exit(1);
    }

    let source_dir = &args[1];
    let dest_dir = &args[2];

    let source_path = Path::new(source_dir).canonicalize()
        .unwrap_or_else(|_| {
            eprintln!("Error: '{}' is not a valid directory", source_dir);
            std::process::exit(1);
        });

    if !source_path.is_dir() {
        eprintln!("Error: '{}' is not a directory", source_dir);
        std::process::exit(1);
    }

    // Canonicalize destination if it exists, otherwise just convert to absolute
    let dest_path = if Path::new(dest_dir).exists() {
        Path::new(dest_dir).canonicalize()
            .unwrap_or_else(|_| {
                eprintln!("Error: Cannot access '{}'", dest_dir);
                std::process::exit(1);
            })
    } else {
        // If destination doesn't exist yet, use the parent's canonical path
        let dest_path_buf = Path::new(dest_dir).to_path_buf();
        if let Some(parent) = dest_path_buf.parent() {
            if parent.exists() {
                parent.canonicalize()
                    .unwrap_or_else(|_| {
                        eprintln!("Error: Cannot access parent of '{}'", dest_dir);
                        std::process::exit(1);
                    })
                    .join(dest_path_buf.file_name().unwrap())
            } else {
                eprintln!("Error: Parent directory of '{}' does not exist", dest_dir);
                std::process::exit(1);
            }
        } else {
            eprintln!("Error: Invalid destination path '{}'", dest_dir);
            std::process::exit(1);
        }
    };

    // Validate source and destination relationship
    if source_path == dest_path {
        eprintln!("Error: Source and destination directories cannot be the same!");
        eprintln!("  Source:      {}", source_path.display());
        eprintln!("  Destination: {}", dest_path.display());
        std::process::exit(1);
    }

    if dest_path.starts_with(&source_path) {
        eprintln!("Error: Destination directory cannot be inside source directory!");
        eprintln!("  Source:      {}", source_path.display());
        eprintln!("  Destination: {}", dest_path.display());
        eprintln!("\nThis would cause thumbnails to be generated for thumbnails.");
        std::process::exit(1);
    }

    if source_path.starts_with(&dest_path) {
        eprintln!("Error: Source directory cannot be inside destination directory!");
        eprintln!("  Source:      {}", source_path.display());
        eprintln!("  Destination: {}", dest_path.display());
        std::process::exit(1);
    }

    // Create destination directory if it doesn't exist
    if !dest_path.exists() {
        println!("Creating destination directory: {}", dest_dir);
        fs::create_dir_all(&dest_path)?;
    }

    println!("🚀 Starting thumbnail generation");
    println!("   Source: {}", source_dir);
    println!("   Destination: {}", dest_dir);
    println!("   Thumbnail size: {}px width", THUMBNAIL_WIDTH);
    println!();

    // Collect all image files
    println!("🔍 Scanning for images...");
    let mut image_files: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(&source_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && is_image_file(path) {
            image_files.push(path.to_path_buf());
        }
    }

    println!("   Found {} image files", image_files.len());
    println!();

    if image_files.is_empty() {
        println!("✅ No images found!");
        return Ok(());
    }

    // Process each image
    let total = image_files.len();
    let mut processed = 0;
    let mut skipped = 0;
    let mut errors = 0;

    for (index, source_file_path) in image_files.iter().enumerate() {
        println!("Progress: {}/{}", index + 1, total);

        let relative_path = source_file_path.strip_prefix(&source_path)?;
        let thumbnail_path = get_thumbnail_path(source_file_path, &source_path, &dest_path);

        println!("📸 {}", relative_path.display());

        match process_image(source_file_path, &thumbnail_path) {
            Ok(_) => {
                if thumbnail_path.metadata()?.len() > 0 {
                    processed += 1;
                } else {
                    skipped += 1;
                }
            }
            Err(e) => {
                eprintln!("❌ Error: {}", e);
                eprintln!("   Skipping and continuing...\n");
                errors += 1;
            }
        }
    }

    println!("\n🎉 All done!");
    println!("   Processed: {}", processed);
    println!("   Skipped: {}", skipped);
    println!("   Errors: {}", errors);

    Ok(())
}
