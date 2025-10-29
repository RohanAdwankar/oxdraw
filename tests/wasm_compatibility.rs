#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use oxdraw::Diagram;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_diagram_parse_and_render() {
        let mermaid_input = r#"graph TD
            A[Start] --> B{Is it working?}
            B -->|Yes| C[Great!]
            B -->|No| D[Debug]
            D --> B"#;

        let diagram = Diagram::parse(mermaid_input).expect("Failed to parse diagram");

        let svg = diagram
            .render_svg("white", None)
            .expect("Failed to render SVG");

        assert!(svg.contains("<svg"));
        assert!(svg.contains("Start"));
        assert!(svg.contains("Great!"));
        assert!(svg.contains("Debug"));
    }

    #[wasm_bindgen_test]
    fn test_simple_flowchart() {
        let mermaid_input = r#"graph LR
            A --> B --> C"#;

        let diagram =
            oxdraw::Diagram::parse(mermaid_input).expect("Failed to parse simple diagram");
        let svg = diagram
            .render_svg("white", None)
            .expect("Failed to render SVG");

        assert!(svg.contains("<svg"));
        assert!(svg.contains("viewBox"));
    }

    #[wasm_bindgen_test]
    fn test_minimal_diagram() {
        let mermaid_input = r#"graph TD
            A"#;

        let diagram =
            oxdraw::Diagram::parse(mermaid_input).expect("Failed to parse minimal diagram");
        let svg = diagram
            .render_svg("white", None)
            .expect("Failed to render minimal SVG");

        assert!(svg.contains("<svg"));
        assert!(svg.contains("A"));
    }
}
