use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

#[test]
fn generates_svg_for_all_fixtures() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let input_dir = manifest_dir.join("tests/input");
    let output_dir = manifest_dir.join("tests/output");

    assert!(
        input_dir.exists(),
        "tests/input directory should exist for CLI fixtures"
    );

    fs::create_dir_all(&output_dir)?;

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
        let output_path = output_dir.join(format!("{stem}.svg"));

        if output_path.exists() {
            fs::remove_file(&output_path)?;
        }

        let mut cmd = Command::cargo_bin("oxdraw")?;
        cmd.arg("--input")
            .arg(&path)
            .arg("--output")
            .arg(&output_path)
            .arg("--output-format")
            .arg("svg");

        cmd.assert().success();

        let svg_contents = fs::read_to_string(&output_path)?;
        assert!(
            svg_contents.contains("<svg"),
            "{} output should contain an <svg> element",
            output_path.display()
        );
    }

    Ok(())
}
