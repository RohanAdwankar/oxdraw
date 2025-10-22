use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

#[test]
fn generates_svg_for_all_fixtures() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let input_dir = manifest_dir.join("tests/input");
    let output_dir = manifest_dir.join("tests/output");
    let svg_output_dir = output_dir.join("svg");
    let png_output_dir = output_dir.join("png");

    assert!(
        input_dir.exists(),
        "tests/input directory should exist for CLI fixtures"
    );

    fs::create_dir_all(&svg_output_dir)?;
    fs::create_dir_all(&png_output_dir)?;

    for entry in fs::read_dir(&input_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("mmd") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("failed to read fixture stem")?;
        let svg_output_path = svg_output_dir.join(format!("{stem}.svg"));
        let png_output_path = png_output_dir.join(format!("{stem}.png"));

        if svg_output_path.exists() {
            fs::remove_file(&svg_output_path)?;
        }

        if png_output_path.exists() {
            fs::remove_file(&png_output_path)?;
        }

        let mut cmd = Command::cargo_bin("oxdraw")?;
        cmd.arg("--input")
            .arg(&path)
            .arg("--output")
            .arg(&svg_output_path)
            .arg("--output-format")
            .arg("svg");

        cmd.assert().success();

        let svg_contents = fs::read_to_string(&svg_output_path)?;
        assert!(
            svg_contents.contains("<svg"),
            "{} output should contain an <svg> element",
            svg_output_path.display()
        );

        let mut png_cmd = Command::cargo_bin("oxdraw")?;
        png_cmd
            .arg("--input")
            .arg(&path)
            .arg("--output")
            .arg(&png_output_path)
            .arg("--png");

        png_cmd.assert().success();

        let png_bytes = fs::read(&png_output_path)?;
        assert!(
            png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "{} output should begin with the PNG magic header",
            png_output_path.display()
        );
    }

    Ok(())
}
