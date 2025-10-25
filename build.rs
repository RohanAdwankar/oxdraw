use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=frontend/out");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing");
    let dist_dir = Path::new(&manifest_dir).join("frontend/out");

    if !dist_dir.join("index.html").is_file() {
        panic!(
            "Missing frontend/out/index.html. Run `npm install` and `npm run build` inside frontend/ before building oxdraw."
        );
    }

    let canonical = dist_dir
        .canonicalize()
        .unwrap_or_else(|_| dist_dir.clone());

    println!(
        "cargo:rustc-env=OXDRAW_BUNDLED_WEB_DIST={}",
        canonical.display()
    );
}