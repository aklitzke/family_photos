use std::path::Path;

fn strip_ext(key: &str) -> String {
    Path::new(key)
        .with_extension("")
        .to_string_lossy()
        .into_owned()
}

fn main() {
    let mut data = scripts::read_history().expect("Failed to read history.toml");

    for image in &mut data.images {
        image.key = strip_ext(&image.key);
    }

    for artifact in &mut data.artifacts {
        artifact.images.front1 = strip_ext(&artifact.images.front1);
        if let Some(ref f2) = artifact.images.front2 {
            artifact.images.front2 = Some(strip_ext(f2));
        }
        if let Some(ref b1) = artifact.images.back1 {
            artifact.images.back1 = Some(strip_ext(b1));
        }
    }

    scripts::write_history(&data).expect("Failed to write history.toml");
    println!("Stripped extensions from {} images and {} artifacts", data.images.len(), data.artifacts.len());
}
