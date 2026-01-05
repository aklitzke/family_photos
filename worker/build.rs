fn main() {
    // Tell Cargo about our custom cfg
    println!("cargo:rustc-check-cfg=cfg(use_local_history_file)");

    // Automatically enable use_local_history_file in debug builds
    // This allows using the bundled history.toml for local development
    #[cfg(debug_assertions)]
    {
        println!("cargo:rustc-cfg=use_local_history_file");
    }

    // In release builds, use_local_history_file is not set,
    // so the code will fetch from GitHub API instead
}
