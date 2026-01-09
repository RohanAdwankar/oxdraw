// With the custom test harness we unfortunately cannot use #![cfg(not(target_arch = "wasm32"))].
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use assert_cmd::cargo::cargo_bin_cmd;
#[cfg(not(target_arch = "wasm32"))]
use libtest_mimic::Failed;
#[cfg(not(target_arch = "wasm32"))]
use similar_asserts::SimpleDiff;
#[cfg(not(target_arch = "wasm32"))]
use tempfile::TempDir;

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
                    let expected_path = manifest_dir
                        .join("tests/expected")
                        .join(format!("{stem}.svg"));
                    move || test_svg(in_path, expected_path)
                }),
                libtest_mimic::Trial::test(format!("png_{stem}"), {
                    let in_path = in_path.clone();
                    move || smoke_test_png(in_path)
                }),
            ]
        })
        .collect();

    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, tests).exit();
}

#[cfg(not(target_arch = "wasm32"))]
fn test_svg(in_path: PathBuf, expected_path: PathBuf) -> Result<(), Failed> {
    let temp_dir = TempDir::new().expect("create temp dir");

    let stem = in_path.file_stem().unwrap().to_str().unwrap();
    let out_path = temp_dir.path().join(format!("{stem}.svg"));

    let mut cmd = cargo_bin_cmd!("oxdraw");
    cmd.arg("--input")
        .arg(in_path)
        .arg("--output")
        .arg(&out_path)
        .arg("--output-format")
        .arg("svg");

    cmd.assert().success();

    let actual = fs::read_to_string(&out_path)?;

    if std::env::var("UPDATE_EXPECTED").is_ok() {
        fs::write(expected_path, actual)?;
        return Ok(());
    }

    let expected = fs::read_to_string(&expected_path)?;

    if expected != actual {
        let diff = format!(
            "{}",
            SimpleDiff::from_str(&actual, &expected, "actual", "expected")
        );
        return Err(diff.into());
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn smoke_test_png(in_path: PathBuf) -> Result<(), Failed> {
    let temp_dir = TempDir::new().expect("create temp dir");

    let stem = in_path.file_stem().unwrap().to_str().unwrap();
    let out_path = temp_dir.path().join(format!("{stem}.png"));

    let mut cmd = cargo_bin_cmd!("oxdraw");
    cmd.arg("--input")
        .arg(&in_path)
        .arg("--output")
        .arg(&out_path)
        .arg("--output-format")
        .arg("png")
        .arg("--scale=1"); // greatly speeds up tests

    cmd.assert().success();

    let png_bytes = fs::read(&out_path)?;
    let starts_with_header = png_bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    if !starts_with_header {
        let _ = temp_dir.keep();
        panic!(
            "{} output should begin with the PNG magic header",
            out_path.display()
        );
    }
    Ok(())
}
