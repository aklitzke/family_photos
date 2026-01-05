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

fn main() -> Result<(), Box<dyn Error>> {
    println!("Reading existing history.toml...");
    let mut history = read_history()?;

    // Build a set of existing artifact front1 keys to avoid duplicates
    let existing_artifact_keys: HashSet<String> = history
        .artifacts
        .iter()
        .map(|artifact| artifact.images.front1.clone())
        .collect();

    println!("Found {} existing artifacts in history.toml", existing_artifact_keys.len());
    println!();

    // Group FastFoto images by their base path and number
    let mut groups: HashMap<(String, String), FastFotoGroup> = HashMap::new();

    for image in &history.images {
        if let Some((base_path, number, variant)) = extract_fastfoto_info(&image.key) {
            let key = (base_path.clone(), number.clone());
            let group = groups.entry(key).or_insert_with(|| FastFotoGroup {
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
        }
    }

    println!("Found {} FastFoto groups", groups.len());
    println!();

    let mut new_artifacts = Vec::new();
    let mut skipped_count = 0;

    for ((base_path, number), group) in groups {
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

                println!("  + Creating artifact: {} ({})",
                         if base_path.is_empty() { "root" } else { &base_path },
                         desc_parts.join(", "));

                Artifact {
                    images: ArtifactImages {
                        front1: variant_a.clone(),
                        front2: base_opt.clone(),
                        back1: back_opt.clone(),
                    },
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

                println!("  + Creating artifact: {} (front1=FastFoto_{}{})",
                         if base_path.is_empty() { "root" } else { &base_path }, number, back_desc);

                Artifact {
                    images: ArtifactImages {
                        front1: base.clone(),
                        front2: None,
                        back1: back_opt.clone(),
                    },
                }
            }

            // Pattern 3: Only _b exists - use it as front1
            (None, None, Some(variant_b)) => {
                if existing_artifact_keys.contains(variant_b) {
                    skipped_count += 1;
                    continue;
                }

                println!("  + Creating artifact: {} (front1=FastFoto_{}_b [back used as front])",
                         if base_path.is_empty() { "root" } else { &base_path }, number);

                Artifact {
                    images: ArtifactImages {
                        front1: variant_b.clone(),
                        front2: None,
                        back1: None,
                    },
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

        new_artifacts.push(artifact);
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
