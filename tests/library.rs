use anyhow::Result;
use oxdraw::{Diagram, EdgeEndpointMarker};

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

    let (node_id, node) = diagram
        .nodes
        .iter()
        .find(|(_, node)| node.image.is_some())
        .expect("expected an image node to be present");
    let image = node
        .image
        .as_ref()
        .expect("expected node image to be parsed");

    assert_eq!(image.mime_type, "image/png");
    assert!(!image.data.is_empty(), "image payload should not be empty");

    let svg = diagram.render_svg("white", None)?;
    assert!(
        svg.contains(&format!("clip-path=\"url(#oxdraw-node-clip-{})\"", node_id)),
        "rendered svg should reference the node clip path"
    );
    assert!(
        svg.contains("data:image/png;base64,"),
        "rendered svg should contain a data URI for the embedded image"
    );

    Ok(())
}

#[test]
fn class_diagram_parses_members_and_relationship_markers() -> Result<()> {
    let definition = include_str!("input/uml.mmd");
    let diagram = Diagram::parse(definition)?;

    let animal = diagram
        .nodes
        .get("Animal")
        .expect("Animal class should be present");
    assert!(animal.label.contains("+int age"));
    assert!(animal.label.contains("+mate()"));

    let inheritance = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Animal" && edge.to == "Duck")
        .expect("Animal inheritance edge should be present");
    assert_eq!(inheritance.marker_start, EdgeEndpointMarker::Triangle);
    assert_eq!(inheritance.marker_end, EdgeEndpointMarker::None);

    Ok(())
}

#[test]
fn class_diagram_parses_composition_and_aggregation_markers() -> Result<()> {
    let definition = r#"
classDiagram
    Car *-- Wheel
    Team o-- Player
"#;

    let diagram = Diagram::parse(definition)?;
    let composition = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Car" && edge.to == "Wheel")
        .expect("composition edge missing");
    assert_eq!(composition.marker_start, EdgeEndpointMarker::Diamond);

    let aggregation = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Team" && edge.to == "Player")
        .expect("aggregation edge missing");
    assert_eq!(aggregation.marker_start, EdgeEndpointMarker::DiamondOpen);

    Ok(())
}
