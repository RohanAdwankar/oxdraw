use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn generates_svg_from_mmd_file() -> Result<(), Box<dyn std::error::Error>> {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mermaid-cli/test-positive/flowchart2.mmd");
    assert!(fixture.exists(), "fixture mermaid diagram should exist");

    let tmp = tempdir()?;
    let output_path = tmp.path().join("diagram.svg");

    let mut cmd = Command::cargo_bin("oxdraw")?;
    cmd.arg("--input")
        .arg(&fixture)
        .arg("--output")
        .arg(&output_path)
        .arg("--output-format")
        .arg("svg");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("diagram"));

    let svg_contents = fs::read_to_string(&output_path)?;
    assert!(
        svg_contents.contains("<svg"),
        "output should contain an <svg> element"
    );

    Ok(())
}
