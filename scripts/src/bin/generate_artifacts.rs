use common::{Artifact, ArtifactImages};
use regex::Regex;
use scripts::{read_history, write_history};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

#[derive(Debug)]
struct FastFotoGroup {
    _base_path: String,
    _number: String,
    base: Option<String>,      // FastFoto_###.ext
    variant_a: Option<String>, // FastFoto_###_a.ext
    variant_b: Option<String>, // FastFoto_###_b.ext
}

fn extract_fastfoto_info(key: &str) -> Option<(String, String, Option<char>)> {
    // Extract directory path, number, and variant (_a, _b, or none)
    let path = Path::new(key);
    let filename = path.file_stem()?.to_str()?;
    let parent = path.parent()?.to_str().unwrap_or("");

    // Match FastFoto_#### or FastFoto_####_a or FastFoto_####_b
    let re = Regex::new(r"^FastFoto_(\d+)(_[ab])?$").ok()?;
    let caps = re.captures(filename)?;

    let number = caps.get(1)?.as_str().to_string();
    let variant = caps.get(2).map(|m| m.as_str().chars().nth(1).unwrap());

    Some((parent.to_string(), number, variant))
}

fn is_timestamp_format(key: &str) -> bool {
    // Check if the filename matches the timestamp format: YYYY-MM-DD-HH-MM-####.ext
    let path = Path::new(key);
    let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    let re = Regex::new(r"^\d{4}-\d{2}-\d{2}-\d{2}-\d{2}-\d{4}$");
    re.map(|r| r.is_match(filename)).unwrap_or(false)
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Reading existing history.toml...");
    let mut history = read_history()?;

    // Find the highest existing ID
    let mut next_id = history
        .artifacts
        .iter()
        .map(|a| a.id)
        .max()
        .unwrap_or(0)
        + 1;

    println!("Found {} existing artifacts in history.toml", history.artifacts.len());
    println!("Next available ID: {}", next_id);
    println!();

    // Build a set of existing artifact front1 keys to avoid duplicates
    let existing_artifact_keys: HashSet<String> = history
        .artifacts
        .iter()
        .map(|artifact| artifact.images.front1().to_string())
        .collect();

    // Group FastFoto images by their base path and number
    let mut fastfoto_groups: HashMap<(String, String), FastFotoGroup> = HashMap::new();
    let mut timestamp_images: Vec<String> = Vec::new();

    for image in &history.images {
        if let Some((base_path, number, variant)) = extract_fastfoto_info(&image.key) {
            let key = (base_path.clone(), number.clone());
            let group = fastfoto_groups.entry(key).or_insert_with(|| FastFotoGroup {
                _base_path: base_path.clone(),
                _number: number.clone(),
                base: None,
                variant_a: None,
                variant_b: None,
            });

            match variant {
                None => group.base = Some(image.key.clone()),
                Some('a') => group.variant_a = Some(image.key.clone()),
                Some('b') => group.variant_b = Some(image.key.clone()),
                _ => {}
            }
        } else if is_timestamp_format(&image.key) {
            timestamp_images.push(image.key.clone());
        }
    }

    println!("Found {} FastFoto groups", fastfoto_groups.len());
    println!("Found {} timestamp format images", timestamp_images.len());
    println!();

    let mut new_artifacts = Vec::new();
    let mut skipped_count = 0;

    // Process FastFoto images
    for ((base_path, number), group) in fastfoto_groups {
        // Determine the artifact configuration based on which variants exist
        let artifact = match (&group.variant_a, &group.base, &group.variant_b) {
            // Pattern 1: _a exists (with optional base as front2 and _b as back1)
            (Some(variant_a), base_opt, back_opt) => {
                if existing_artifact_keys.contains(variant_a) {
                    skipped_count += 1;
                    continue;
                }

                let mut desc_parts = vec![format!("front1=FastFoto_{}_a", number)];
                if base_opt.is_some() {
                    desc_parts.push(format!("front2=FastFoto_{}", number));
                }
                if back_opt.is_some() {
                    desc_parts.push(format!("back1=FastFoto_{}_b", number));
                }

                println!("  + Creating artifact #{}: {} ({})",
                         next_id,
                         if base_path.is_empty() { "root" } else { &base_path },
                         desc_parts.join(", "));

                Artifact {
                    id: next_id,
                    images: ArtifactImages {
                        fronts: std::iter::once(variant_a.clone())
                            .chain(base_opt.clone())
                            .collect(),
                        backs: back_opt.clone().into_iter().collect(),
                    },
                    updates: vec![],
                }
            }

            // Pattern 2: Base exists without _a (with optional _b as back1)
            (None, Some(base), back_opt) => {
                if existing_artifact_keys.contains(base) {
                    skipped_count += 1;
                    continue;
                }

                let back_desc = if back_opt.is_some() {
                    format!(", back1=FastFoto_{}_b", number)
                } else {
                    String::new()
                };

                println!("  + Creating artifact #{}: {} (front1=FastFoto_{}{})",
                         next_id,
                         if base_path.is_empty() { "root" } else { &base_path }, number, back_desc);

                Artifact {
                    id: next_id,
                    images: ArtifactImages {
                        fronts: vec![base.clone()],
                        backs: back_opt.clone().into_iter().collect(),
                    },
                    updates: vec![],
                }
            }

            // Pattern 3: Only _b exists - use it as front1
            (None, None, Some(variant_b)) => {
                if existing_artifact_keys.contains(variant_b) {
                    skipped_count += 1;
                    continue;
                }

                println!("  + Creating artifact #{}: {} (front1=FastFoto_{}_b [back used as front])",
                         next_id,
                         if base_path.is_empty() { "root" } else { &base_path }, number);

                Artifact {
                    id: next_id,
                    images: ArtifactImages {
                        fronts: vec![variant_b.clone()],
                        backs: vec![],
                    },
                    updates: vec![],
                }
            }

            // Pattern 4: No images at all - shouldn't happen
            (None, None, None) => {
                panic!(
                    "FastFoto group with no images: {}/FastFoto_{}",
                    if base_path.is_empty() { "root" } else { &base_path },
                    number
                );
            }
        };

        next_id += 1;
        new_artifacts.push(artifact);
    }

    // Process timestamp format images
    for timestamp_key in timestamp_images {
        if existing_artifact_keys.contains(&timestamp_key) {
            skipped_count += 1;
            continue;
        }

        let path = Path::new(&timestamp_key);
        let parent = path.parent().and_then(|p| p.to_str()).unwrap_or("root");
        let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or(&timestamp_key);

        println!("  + Creating artifact #{}: {} (front1={})",
                 next_id, parent, filename);

        new_artifacts.push(Artifact {
            id: next_id,
            images: ArtifactImages {
                fronts: vec![timestamp_key],
                backs: vec![],
            },
            updates: vec![],
        });

        next_id += 1;
    }

    println!();
    println!("Summary:");
    println!("  New artifacts created: {}", new_artifacts.len());
    println!("  Artifacts skipped (already exist): {}", skipped_count);

    if new_artifacts.is_empty() {
        println!();
        println!("No new artifacts to add. history.toml is up to date.");
        return Ok(());
    }

    // Add new artifacts to history
    history.artifacts.extend(new_artifacts);

    println!();
    println!("Writing updated history.toml...");
    write_history(&history)?;

    println!("Done! history.toml now contains {} total artifacts.", history.artifacts.len());

    Ok(())
}
