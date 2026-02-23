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
        for front in &mut artifact.images.fronts {
            *front = strip_ext(front);
        }
        for back in &mut artifact.images.backs {
            *back = strip_ext(back);
        }
    }

    scripts::write_history(&data).expect("Failed to write history.toml");
    println!("Stripped extensions from {} images and {} artifacts", data.images.len(), data.artifacts.len());
}
