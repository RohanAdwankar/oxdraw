use anyhow::Result;
use oxdraw::Diagram;

#[test]
fn diagram_parse_and_render_svg() -> Result<()> {
    let definition = r#"
        graph TD
            A[Start] -->|process| B[End]
    "#;

    let diagram = Diagram::parse(definition)?;
    let svg = diagram.render_svg("white", None)?;

    assert!(
        svg.contains("<svg"),
        "rendered svg should contain root element"
    );
    assert!(svg.contains("Start"), "node labels should appear in output");

    Ok(())
}

#[test]
fn diagram_render_png_has_png_header() -> Result<()> {
    let definition = r#"
        graph LR
            Start --> Finish
    "#;

    let diagram = Diagram::parse(definition)?;
    let png = diagram.render_png("white", None, 2.0)?;

    const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    assert!(
        png.starts_with(PNG_MAGIC),
        "rendered png should start with PNG header"
    );

    Ok(())
}
