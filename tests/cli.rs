// With the custom test harness we unfortunately cannot use #![cfg(not(target_arch = "wasm32"))].
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use assert_cmd::cargo::cargo_bin_cmd;
#[cfg(not(target_arch = "wasm32"))]
use libtest_mimic::Failed;

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let input_dir = manifest_dir.join("tests/input");

    let in_paths: Vec<_> = fs::read_dir(&input_dir)
        .expect("read dir")
        .flatten()
        .map(|entry| entry.path())
        .collect();

    assert!(in_paths.len() > 0, "expected tests/input to contain files");

    assert_eq!(
        in_paths
            .iter()
            .filter(|input| input.extension().and_then(OsStr::to_str) != Some("mmd"))
            .map(|input| input.file_name().unwrap())
            .collect::<Vec<_>>(),
        Vec::<&OsStr>::new(),
        "expected files in tests/input/ to have the .mmd extension"
    );

    let tests: Vec<_> = in_paths
        .into_iter()
        .flat_map(|in_path| {
            let stem = in_path.file_stem().unwrap().to_str().unwrap();
            [
                libtest_mimic::Trial::test(format!("svg_{stem}"), {
                    let in_path = in_path.clone();
                    let out_path = manifest_dir
                        .join("tests/output/svg")
                        .join(format!("{stem}.svg"));
                    move || smoke_test_svg(in_path, out_path)
                }),
                libtest_mimic::Trial::test(format!("png_{stem}"), {
                    let in_path = in_path.clone();
                    let out_path = manifest_dir
                        .join("tests/output/png")
                        .join(format!("{stem}.png"));
                    move || smoke_test_png(in_path, out_path)
                }),
            ]
        })
        .collect();

    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, tests).exit();
}

#[cfg(not(target_arch = "wasm32"))]
fn smoke_test_svg(in_path: PathBuf, out_path: PathBuf) -> Result<(), Failed> {
    if out_path.exists() {
        fs::remove_file(&out_path)?;
    }

    let mut cmd = cargo_bin_cmd!("oxdraw");
    cmd.arg("--input")
        .arg(in_path)
        .arg("--output")
        .arg(&out_path)
        .arg("--output-format")
        .arg("svg");

    cmd.assert().success();

    let svg_contents = fs::read_to_string(&out_path)?;
    assert!(
        svg_contents.contains("<svg"),
        "{} output should contain an <svg> element",
        out_path.display()
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn smoke_test_png(in_path: PathBuf, out_path: PathBuf) -> Result<(), Failed> {
    if out_path.exists() {
        fs::remove_file(&out_path)?;
    }

    let mut cmd = cargo_bin_cmd!("oxdraw");
    cmd.arg("--input")
        .arg(in_path)
        .arg("--output")
        .arg(&out_path)
        .arg("--output-format")
        .arg("png");

    cmd.assert().success();

    let png_bytes = fs::read(&out_path)?;
    assert!(
        png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{} output should begin with the PNG magic header",
        out_path.display()
    );
    Ok(())
}
