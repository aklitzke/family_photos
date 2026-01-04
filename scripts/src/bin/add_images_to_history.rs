use common::ImageMetadata;
use scripts::{read_history, write_history};
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: cargo run --bin add_images_to_history <source_directory> [base_path_to_strip] [prefix]");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  cargo run --bin add_images_to_history /path/to/images");
        eprintln!("  cargo run --bin add_images_to_history /path/to/images /path/to");
        eprintln!("  cargo run --bin add_images_to_history /path/to/images \"\" \"Family History/\"");
        eprintln!("  cargo run --bin add_images_to_history /path/to/images /path/to \"Family History/\"");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  base_path_to_strip - Path prefix to remove from file paths (optional)");
        eprintln!("  prefix             - String to add to the beginning of each key (optional)");
        eprintln!("                       Use empty string \"\" to skip base_path_to_strip if you only want prefix");
        std::process::exit(1);
    }

    let source_dir = PathBuf::from(&args[1]);
    let base_path_to_strip = if args.len() >= 3 && !args[2].is_empty() {
        Some(PathBuf::from(&args[2]))
    } else {
        None
    };
    let prefix = if args.len() >= 4 {
        args[3].clone()
    } else {
        String::new()
    };

    if !source_dir.exists() {
        eprintln!("Error: Source directory does not exist: {}", source_dir.display());
        std::process::exit(1);
    }

    if !source_dir.is_dir() {
        eprintln!("Error: Path is not a directory: {}", source_dir.display());
        std::process::exit(1);
    }

    println!("Reading existing history.toml...");
    let mut history = read_history()?;

    // Build a set of existing image keys for quick lookup
    let existing_keys: HashSet<String> = history
        .images
        .iter()
        .map(|img| img.key.clone())
        .collect();

    println!("Found {} existing images in history.toml", existing_keys.len());
    println!();
    println!("Configuration:");
    println!("  Source directory: {}", source_dir.display());
    if let Some(base) = &base_path_to_strip {
        println!("  Base path to strip: {}", base.display());
    }
    if !prefix.is_empty() {
        println!("  Prefix to add: \"{}\"", prefix);
    }
    println!();
    println!("Scanning for images...");

    // Walk the directory and find all image files
    let mut new_images = Vec::new();
    let mut skipped_count = 0;

    for entry in WalkDir::new(&source_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if !path.is_file() || !is_image_file(path) {
            continue;
        }

        // Generate the key for this image
        let relative_path = if let Some(base) = &base_path_to_strip {
            // Strip the base path
            match path.strip_prefix(base) {
                Ok(relative) => relative.to_string_lossy().to_string(),
                Err(_) => {
                    eprintln!("Warning: Could not strip base path from: {}", path.display());
                    continue;
                }
            }
        } else {
            // Use the path relative to the source directory
            match path.strip_prefix(&source_dir) {
                Ok(relative) => relative.to_string_lossy().to_string(),
                Err(_) => {
                    eprintln!("Warning: Could not make path relative: {}", path.display());
                    continue;
                }
            }
        };

        // Apply prefix if provided
        let key = format!("{}{}", prefix, relative_path);

        // Check if this key already exists
        if existing_keys.contains(&key) {
            skipped_count += 1;
            continue;
        }

        println!("  + {}", key);
        new_images.push(ImageMetadata {
            key,
            rotation: None,
        });
    }

    println!();
    println!("Summary:");
    println!("  New images found: {}", new_images.len());
    println!("  Images skipped (already in history): {}", skipped_count);

    if new_images.is_empty() {
        println!();
        println!("No new images to add. history.toml is up to date.");
        return Ok(());
    }

    // Add new images to history
    history.images.extend(new_images);

    // Sort images by key for consistent ordering
    history.images.sort_by(|a, b| a.key.cmp(&b.key));

    println!();
    println!("Writing updated history.toml...");
    write_history(&history)?;

    println!("Done! history.toml now contains {} total images.", history.images.len());

    Ok(())
}
