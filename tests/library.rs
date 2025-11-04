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

#[test]
fn diagram_parses_image_comments() -> Result<()> {
    let definition = include_str!("input/image_node.mmd");
    let diagram = Diagram::parse(definition)?;

    let node = diagram
        .nodes
        .get("IMG")
        .expect("expected IMG node to be present");
    let image = node
        .image
        .as_ref()
        .expect("expected node image to be parsed");

    assert_eq!(image.mime_type, "image/png");
    assert!(!image.data.is_empty(), "image payload should not be empty");

    let svg = diagram.render_svg("white", None)?;
    assert!(
        svg.contains("clip-path=\"url(#oxdraw-node-clip-IMG)\""),
        "rendered svg should reference the node clip path"
    );
    assert!(
        svg.contains("data:image/png;base64,"),
        "rendered svg should contain a data URI for the embedded image"
    );

    Ok(())
}
