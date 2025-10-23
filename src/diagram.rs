use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as FmtWrite;
use tiny_skia::{Pixmap, Transform};

use crate::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutOverrides {
    #[serde(default)]
    pub nodes: HashMap<String, Point>,
    #[serde(default)]
    pub edges: HashMap<String, EdgeOverride>,
    #[serde(default)]
    pub node_styles: HashMap<String, NodeStyleOverride>,
    #[serde(default)]
    pub edge_styles: HashMap<String, EdgeStyleOverride>,
}

#[derive(Debug, Clone)]
pub struct Subgraph {
    pub id: String,
    pub label: String,
    pub nodes: Vec<String>,
    pub children: Vec<Subgraph>,
    pub order: usize,
}

#[derive(Debug, Clone)]
struct SubgraphBuilder {
    id: String,
    label: String,
    nodes: Vec<String>,
    children: Vec<SubgraphBuilder>,
    order: usize,
}

impl SubgraphBuilder {
    fn new(id: String, label: String, order: usize) -> Self {
        Self {
            id,
            label,
            nodes: Vec::new(),
            children: Vec::new(),
            order,
        }
    }

    fn into_subgraph(self) -> Subgraph {
        Subgraph {
            id: self.id,
            label: self.label,
            nodes: self.nodes,
            children: self
                .children
                .into_iter()
                .map(SubgraphBuilder::into_subgraph)
                .collect(),
            order: self.order,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagram {
    pub direction: Direction,
    pub nodes: HashMap<String, Node>,
    pub order: Vec<String>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
    pub node_membership: HashMap<String, Vec<String>>,
}

impl LayoutOverrides {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.edges.is_empty()
            && self.node_styles.is_empty()
            && self.edge_styles.is_empty()
    }

    pub fn prune(&mut self, nodes: &HashSet<String>, edges: &HashSet<String>) {
        self.nodes.retain(|id, _| nodes.contains(id));
        self.edges.retain(|id, _| edges.contains(id));
        self.node_styles.retain(|id, _| nodes.contains(id));
        self.edge_styles.retain(|id, _| edges.contains(id));
    }
}

impl Diagram {
    pub fn parse(definition: &str) -> Result<Self> {
        let mut lines = definition
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with("%%"));

        let header = lines
            .next()
            .ok_or_else(|| anyhow!("diagram definition must start with a 'graph' declaration"))?;

        let direction = parse_graph_header(header)?;

        let mut nodes = HashMap::new();
        let mut order = Vec::new();
        let mut edges = Vec::new();
        let mut node_membership: HashMap<String, Vec<String>> = HashMap::new();
        let mut subgraph_stack: Vec<SubgraphBuilder> = Vec::new();
        let mut top_subgraphs: Vec<SubgraphBuilder> = Vec::new();
        let mut seen_subgraph_ids: HashSet<String> = HashSet::new();
        let mut subgraph_counter = 0_usize;

        for line in lines {
            if let Some(rest) = line.strip_prefix("subgraph") {
                let (id, label) = parse_subgraph_header(rest)?;
                if !seen_subgraph_ids.insert(id.clone()) {
                    bail!("duplicate subgraph identifier '{id}'");
                }
                let builder = SubgraphBuilder::new(id, label, subgraph_counter);
                subgraph_counter += 1;
                subgraph_stack.push(builder);
                continue;
            }

            if line.eq_ignore_ascii_case("end") {
                let builder = subgraph_stack
                    .pop()
                    .ok_or_else(|| anyhow!("encountered 'end' without matching 'subgraph'"))?;
                if let Some(parent) = subgraph_stack.last_mut() {
                    parent.children.push(builder);
                } else {
                    top_subgraphs.push(builder);
                }
                continue;
            }

            if let Some(edge) = parse_edge_line(
                line,
                &mut nodes,
                &mut order,
                &mut node_membership,
                &mut subgraph_stack,
            )? {
                edges.push(edge);
                continue;
            }

            if parse_node_line(
                line,
                &mut nodes,
                &mut order,
                &mut node_membership,
                &mut subgraph_stack,
            )? {
                continue;
            }
        }

        if let Some(unclosed) = subgraph_stack.last() {
            bail!("subgraph '{}' missing closing 'end'", unclosed.id);
        }

        if nodes.is_empty() {
            bail!("diagram does not declare any nodes");
        }

        Ok(Self {
            direction,
            nodes,
            order,
            edges,
            subgraphs: top_subgraphs
                .into_iter()
                .map(SubgraphBuilder::into_subgraph)
                .collect(),
            node_membership,
        })
    }

    pub fn render_svg(
        &self,
        background: &str,
        overrides: Option<&LayoutOverrides>,
    ) -> Result<String> {
        let layout = self.layout(overrides)?;
        let geometry = align_geometry(
            &layout.final_positions,
            &layout.final_routes,
            &self.edges,
            &self.subgraphs,
        )?;

        let mut svg = String::new();
        write!(
            svg,
            r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" viewBox="0 0 {:.0} {:.0}" font-family="Inter, system-ui, sans-serif">
  <defs>
        <marker id="arrow-end" markerWidth="8" markerHeight="8" refX="6" refY="4" orient="auto" markerUnits="strokeWidth">
            <path d="M1,1 L6,4 L1,7 z" fill="context-stroke" />
        </marker>
        <marker id="arrow-start" markerWidth="8" markerHeight="8" refX="2" refY="4" orient="auto" markerUnits="strokeWidth">
            <path d="M7,1 L2,4 L7,7 z" fill="context-stroke" />
        </marker>
  </defs>
  <rect width="100%" height="100%" fill="{}" />
"##,
            geometry.width,
            geometry.height,
            geometry.width,
            geometry.height,
            escape_xml(background)
        )?;

        let subgraph_fill = "#edf2f7";
        let subgraph_stroke = "#a0aec0";
        let subgraph_label = "#2d3748";

        for subgraph in &geometry.subgraphs {
            write!(
                svg,
                "  <g class=\"subgraph\" data-id=\"{}\">\n    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"16\" ry=\"16\" fill=\"{}\" fill-opacity=\"0.7\" stroke=\"{}\" stroke-width=\"1.5\" />\n    <text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"14\" font-weight=\"600\" text-anchor=\"start\" dominant-baseline=\"hanging\">{}</text>\n  </g>\n",
                escape_xml(&subgraph.id),
                subgraph.x,
                subgraph.y,
                subgraph.width,
                subgraph.height,
                subgraph_fill,
                subgraph_stroke,
                subgraph.label_x,
                subgraph.label_y,
                subgraph_label,
                escape_xml(&subgraph.label)
            )?;
        }

        for edge in &self.edges {
            let id = edge_identifier(edge);
            let route = geometry
                .edges
                .get(&id)
                .cloned()
                .ok_or_else(|| anyhow!("missing geometry for edge '{id}'"))?;

            let mut stroke_color = "#2d3748".to_string();
            let mut effective_kind = edge.kind;
            let mut arrow_direction = EdgeArrowDirection::Forward;

            if let Some(overrides) = overrides {
                if let Some(style) = overrides.edge_styles.get(&id) {
                    if let Some(line) = style.line {
                        effective_kind = line;
                    }
                    if let Some(color) = &style.color {
                        stroke_color = color.clone();
                    }
                    if let Some(direction) = style.arrow {
                        arrow_direction = direction;
                    }
                }
            }

            let dash_attr = if effective_kind == EdgeKind::Dashed {
                " stroke-dasharray=\"8 6\""
            } else {
                ""
            };

            let marker_start_attr = if arrow_direction.marker_start() {
                " marker-start=\"url(#arrow-start)\""
            } else {
                ""
            };

            let marker_end_attr = if arrow_direction.marker_end() {
                " marker-end=\"url(#arrow-end)\""
            } else {
                ""
            };

            if route.len() == 2 {
                let a = route[0];
                let b = route[1];
                write!(
                    svg,
                    "  <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"{}{}{} />\n",
                    a.x, a.y, b.x, b.y, stroke_color, marker_start_attr, marker_end_attr, dash_attr
                )?;
            } else {
                let points = route
                    .iter()
                    .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(
                    svg,
                    "  <polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\"{}{}{} />\n",
                    points, stroke_color, marker_start_attr, marker_end_attr, dash_attr
                )?;
            }

            if let Some(label) = &edge.label {
                let label_center = label_center_for_route(&route);
                let lines = normalize_label_lines(label);

                if lines.is_empty() {
                    continue;
                }

                let (box_width, box_height) = measure_label_box(&lines);
                let rect_x = label_center.x - box_width / 2.0;
                let rect_y = label_center.y - box_height / 2.0;

                write!(
                    svg,
                    "  <g pointer-events=\"none\">\n    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"6\" ry=\"6\" fill=\"white\" fill-opacity=\"0.96\" stroke=\"{}\" stroke-width=\"1\" />\n",
                    rect_x, rect_y, box_width, box_height, stroke_color
                )?;

                if lines.len() <= 1 {
                    if let Some(single_line) = lines.first() {
                        write!(
                            svg,
                            "    <text x=\"{:.1}\" y=\"{:.1}\" fill=\"#2d3748\" font-size=\"13\" text-anchor=\"middle\" dominant-baseline=\"middle\" xml:space=\"preserve\">{}</text>\n",
                            label_center.x,
                            label_center.y,
                            escape_xml(single_line)
                        )?;
                    }
                } else {
                    let start_y =
                        label_center.y - EDGE_LABEL_LINE_HEIGHT * (lines.len() as f32 - 1.0) / 2.0;
                    write!(
                        svg,
                        "    <text x=\"{:.1}\" fill=\"#2d3748\" font-size=\"13\" text-anchor=\"middle\" xml:space=\"preserve\">\n",
                        label_center.x
                    )?;
                    for (idx, line_text) in lines.iter().enumerate() {
                        let line_y = start_y + EDGE_LABEL_LINE_HEIGHT * idx as f32;
                        write!(
                            svg,
                            "      <tspan x=\"{:.1}\" y=\"{:.1}\" dominant-baseline=\"middle\">{}</tspan>\n",
                            label_center.x,
                            line_y,
                            escape_xml(line_text)
                        )?;
                    }
                    svg.push_str("    </text>\n");
                }

                svg.push_str("  </g>\n");
            }
        }

        for (id, node) in &self.nodes {
            let position = geometry
                .positions
                .get(id)
                .copied()
                .ok_or_else(|| anyhow!("missing geometry for node '{id}'"))?;

            let mut fill_color = node.shape.default_fill_color().to_string();
            let mut stroke_color = "#2d3748".to_string();
            let mut text_color = "#1a202c".to_string();

            if let Some(overrides) = overrides {
                if let Some(style) = overrides.node_styles.get(id) {
                    if let Some(fill) = &style.fill {
                        fill_color = fill.clone();
                    }
                    if let Some(stroke) = &style.stroke {
                        stroke_color = stroke.clone();
                    }
                    if let Some(text) = &style.text {
                        text_color = text.clone();
                    }
                }
            }

            match node.shape {
                NodeShape::Rectangle => write!(
                    svg,
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" ry=\"8\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\" />\n",
                    position.x - NODE_WIDTH / 2.0,
                    position.y - NODE_HEIGHT / 2.0,
                    NODE_WIDTH,
                    NODE_HEIGHT,
                    fill_color,
                    stroke_color
                )?,
                NodeShape::Stadium => write!(
                    svg,
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"30\" ry=\"30\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\" />\n",
                    position.x - NODE_WIDTH / 2.0,
                    position.y - NODE_HEIGHT / 2.0,
                    NODE_WIDTH,
                    NODE_HEIGHT,
                    fill_color,
                    stroke_color
                )?,
                NodeShape::Circle => write!(
                    svg,
                    "  <ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\" />\n",
                    position.x,
                    position.y,
                    NODE_WIDTH / 2.0,
                    NODE_HEIGHT / 2.0,
                    fill_color,
                    stroke_color
                )?,
                NodeShape::Diamond => {
                    let half_w = NODE_WIDTH / 2.0;
                    let half_h = NODE_HEIGHT / 2.0;
                    write!(
                        svg,
                        "  <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\" />\n",
                        position.x,
                        position.y - half_h,
                        position.x + half_w,
                        position.y,
                        position.x,
                        position.y + half_h,
                        position.x - half_w,
                        position.y,
                        fill_color,
                        stroke_color
                    )?;
                }
            }

            write!(
                svg,
                "  <text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\" font-size=\"14\" text-anchor=\"middle\" dominant-baseline=\"middle\">{}</text>\n",
                position.x,
                position.y,
                text_color,
                escape_xml(&node.label)
            )?;
        }

        svg.push_str("</svg>\n");
        Ok(svg)
    }

    pub fn render_png(
        &self,
        background: &str,
        overrides: Option<&LayoutOverrides>,
        scale: f32,
    ) -> Result<Vec<u8>> {
        if scale <= 0.0 {
            bail!("scale must be greater than zero when rendering PNG output");
        }

        let svg = self.render_svg(background, overrides)?;

        let mut options = resvg::usvg::Options::default();
        options.font_family = "Inter".to_string();
        options.fontdb_mut().load_system_fonts();

        let tree = resvg::usvg::Tree::from_str(&svg, &options)
            .map_err(|err| anyhow!("failed to parse generated SVG for PNG export: {err}"))?;

        let size = tree.size().to_int_size();
        let width = size.width();
        let height = size.height();

        let scaled_width = ((width as f32) * scale).ceil();
        let scaled_height = ((height as f32) * scale).ceil();

        if !scaled_width.is_finite() || !scaled_height.is_finite() {
            bail!("scaled dimensions are not finite; try a smaller scale factor");
        }

        if scaled_width < 1.0 || scaled_height < 1.0 {
            bail!("scaled dimensions collapsed below 1px; try a larger scale factor");
        }

        if scaled_width > u32::MAX as f32 || scaled_height > u32::MAX as f32 {
            bail!("scaled dimensions exceed supported limits; try a smaller scale factor");
        }

        let scaled_width = scaled_width as u32;
        let scaled_height = scaled_height as u32;

        let mut pixmap = Pixmap::new(scaled_width, scaled_height).ok_or_else(|| {
            anyhow!("failed to allocate {scaled_width}x{scaled_height} surface for PNG export")
        })?;

        let transform = Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let png_data = pixmap
            .encode_png()
            .map_err(|err| anyhow!("failed to encode PNG output: {err}"))?;

        Ok(png_data)
    }

    pub fn layout(&self, overrides: Option<&LayoutOverrides>) -> Result<LayoutComputation> {
        let mut auto = self.compute_auto_layout();
        self.separate_top_level_subgraphs(&mut auto.positions);
        auto.size = compute_canvas_size_for_positions(&auto.positions);
        let mut final_positions = auto.positions.clone();

        if let Some(overrides) = overrides {
            for (id, point) in &overrides.nodes {
                if final_positions.contains_key(id) {
                    final_positions.insert(id.clone(), *point);
                }
            }
        }

        let auto_routes = self.compute_routes(&auto.positions, None)?;
        let final_routes = self.compute_routes(&final_positions, overrides)?;

        Ok(LayoutComputation {
            auto_positions: auto.positions,
            auto_routes,
            auto_size: auto.size,
            final_positions,
            final_routes,
        })
    }

    fn compute_auto_layout(&self) -> AutoLayout {
        if self.order.is_empty() {
            let size = CanvasSize {
                width: START_OFFSET * 2.0 + NODE_WIDTH,
                height: START_OFFSET * 2.0 + NODE_HEIGHT,
            };
            return AutoLayout {
                positions: HashMap::new(),
                size,
            };
        }

        let mut levels: HashMap<String, usize> =
            self.nodes.keys().cloned().map(|id| (id, 0_usize)).collect();

        let mut indegree: HashMap<String, usize> =
            self.nodes.keys().cloned().map(|id| (id, 0_usize)).collect();

        for edge in &self.edges {
            *indegree.entry(edge.to.clone()).or_insert(0) += 1;
        }

        let mut queue: VecDeque<String> = VecDeque::new();
        for id in &self.order {
            if indegree.get(id).copied().unwrap_or(0) == 0 {
                queue.push_back(id.clone());
            }
        }

        let mut visited: HashSet<String> = HashSet::new();

        while let Some(node_id) = queue.pop_front() {
            visited.insert(node_id.clone());
            let node_level = *levels.get(&node_id).unwrap_or(&0);

            for edge in self.edges.iter().filter(|edge| edge.from == node_id) {
                let target_id = edge.to.clone();
                let entry = levels.entry(target_id.clone()).or_insert(0);
                if *entry < node_level + 1 {
                    *entry = node_level + 1;
                }

                if let Some(degree) = indegree.get_mut(&target_id) {
                    if *degree > 0 {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(target_id.clone());
                        }
                    }
                }
            }
        }

        if visited.len() != self.nodes.len() {
            for id in &self.order {
                if visited.contains(id) {
                    continue;
                }
                let mut max_parent = 0_usize;
                let mut has_parent = false;
                for edge in self.edges.iter().filter(|edge| edge.to == *id) {
                    has_parent = true;
                    let parent_level = *levels.get(&edge.from).unwrap_or(&0);
                    max_parent = max_parent.max(parent_level + 1);
                }
                levels.insert(id.clone(), if has_parent { max_parent } else { 0 });
            }
        }

        let mut layers_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for id in &self.order {
            let level = *levels.get(id).unwrap_or(&0);
            layers_map.entry(level).or_default().push(id.clone());
        }

        if layers_map.is_empty() {
            layers_map.insert(0, self.order.clone());
        }

        let layers: Vec<Vec<String>> = layers_map.into_values().collect();
        let level_count = layers.len().max(1);
        let max_per_level = layers
            .iter()
            .map(|layer| layer.len())
            .max()
            .unwrap_or(1)
            .max(1);

        let mut positions = HashMap::new();

        let (width, height) = match self.direction {
            Direction::TopDown | Direction::BottomTop => {
                let inner_width = NODE_WIDTH + NODE_SPACING * ((max_per_level - 1) as f32);
                let inner_height = NODE_HEIGHT + NODE_SPACING * ((level_count - 1) as f32);
                let width = inner_width + START_OFFSET * 2.0;
                let height = inner_height + START_OFFSET * 2.0;

                let vertical_span = NODE_SPACING * ((level_count - 1) as f32);
                let start_y = START_OFFSET + (inner_height - vertical_span) / 2.0;

                for (idx, nodes) in layers.iter().enumerate() {
                    let row_index = if matches!(self.direction, Direction::BottomTop) {
                        level_count - 1 - idx
                    } else {
                        idx
                    } as f32;
                    let y = start_y + row_index * NODE_SPACING;

                    let span = NODE_SPACING * ((nodes.len().saturating_sub(1)) as f32);
                    let start_x = START_OFFSET + (inner_width - span) / 2.0;

                    for (col_idx, id) in nodes.iter().enumerate() {
                        let x = start_x + col_idx as f32 * NODE_SPACING;
                        positions.insert(id.clone(), Point { x, y });
                    }
                }

                (width, height)
            }
            Direction::LeftRight | Direction::RightLeft => {
                let inner_width = NODE_WIDTH + NODE_SPACING * ((level_count - 1) as f32);
                let inner_height = NODE_HEIGHT + NODE_SPACING * ((max_per_level - 1) as f32);
                let width = inner_width + START_OFFSET * 2.0;
                let height = inner_height + START_OFFSET * 2.0;

                let horizontal_span = NODE_SPACING * ((level_count - 1) as f32);
                let start_x = START_OFFSET + (inner_width - horizontal_span) / 2.0;

                for (idx, nodes) in layers.iter().enumerate() {
                    let column_index = if matches!(self.direction, Direction::RightLeft) {
                        level_count - 1 - idx
                    } else {
                        idx
                    } as f32;
                    let x = start_x + column_index * NODE_SPACING;

                    let span = NODE_SPACING * ((nodes.len().saturating_sub(1)) as f32);
                    let start_y = START_OFFSET + (inner_height - span) / 2.0;

                    for (row_idx, id) in nodes.iter().enumerate() {
                        let y = start_y + row_idx as f32 * NODE_SPACING;
                        positions.insert(id.clone(), Point { x, y });
                    }
                }

                (width, height)
            }
        };

        AutoLayout {
            positions,
            size: CanvasSize { width, height },
        }
    }

    fn separate_top_level_subgraphs(&self, positions: &mut HashMap<String, Point>) {
        if self.subgraphs.is_empty() {
            return;
        }

        let mut placed_bounds: Vec<Rect> = Vec::new();

        for subgraph in &self.subgraphs {
            let nodes = gather_subgraph_nodes(subgraph);
            if nodes.is_empty() {
                continue;
            }

            let mut bounds = match compute_group_bounds(&nodes, positions) {
                Some(bounds) => bounds,
                None => continue,
            };

            let mut required_shift = 0.0_f32;
            loop {
                let shifted = Rect {
                    min_x: bounds.min_x + required_shift,
                    max_x: bounds.max_x + required_shift,
                    min_y: bounds.min_y,
                    max_y: bounds.max_y,
                };

                let mut overlap_shift = None;
                for placed in &placed_bounds {
                    if rects_intersect_with_margin(&shifted, placed, SUBGRAPH_SEPARATION) {
                        let candidate = placed.max_x + SUBGRAPH_SEPARATION - bounds.min_x;
                        overlap_shift = Some(candidate.max(required_shift));
                        break;
                    }
                }

                if let Some(new_shift) = overlap_shift {
                    if (new_shift - required_shift).abs() < f32::EPSILON {
                        required_shift = new_shift + SUBGRAPH_SEPARATION;
                    } else {
                        required_shift = new_shift;
                    }
                    continue;
                }

                if required_shift.abs() > f32::EPSILON {
                    offset_nodes(positions, &nodes, required_shift, 0.0);
                    bounds.min_x += required_shift;
                    bounds.max_x += required_shift;
                }

                placed_bounds.push(bounds);
                break;
            }
        }
    }

    fn compute_routes(
        &self,
        positions: &HashMap<String, Point>,
        overrides: Option<&LayoutOverrides>,
    ) -> Result<HashMap<String, Vec<Point>>> {
        let mut routes = HashMap::new();
        let mut edge_ids = Vec::with_capacity(self.edges.len());
        let mut pairings: HashMap<(String, String), Vec<(usize, bool)>> = HashMap::new();

        let mut node_bounds: HashMap<String, NodeBoundary> = HashMap::new();
        for (id, point) in positions {
            let node = self
                .nodes
                .get(id)
                .ok_or_else(|| anyhow!("node '{id}' missing definition"))?;
            node_bounds.insert(id.clone(), NodeBoundary::new(*point, node.shape));
        }

        for (idx, edge) in self.edges.iter().enumerate() {
            let edge_id = edge_identifier(edge);
            edge_ids.push(edge_id);

            let mut a = edge.from.clone();
            let mut b = edge.to.clone();
            let mut is_forward = true;
            if a > b {
                std::mem::swap(&mut a, &mut b);
                is_forward = false;
            }

            pairings.entry((a, b)).or_default().push((idx, is_forward));
        }

        let mut auto_points: HashMap<usize, Vec<Point>> = HashMap::new();

        let has_override = |edge_idx: usize| -> bool {
            overrides.map_or(false, |ov| ov.edges.contains_key(&edge_ids[edge_idx]))
        };

        for ((a, b), entries) in pairings {
            if a == b || entries.len() < 2 {
                continue;
            }

            let mut forward = Vec::new();
            let mut backward = Vec::new();

            for (idx, is_forward) in entries {
                if is_forward {
                    forward.push(idx);
                } else {
                    backward.push(idx);
                }
            }

            if forward.is_empty() || backward.is_empty() {
                continue;
            }

            let from = *positions
                .get(&a)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", a))?;
            let to = *positions
                .get(&b)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", b))?;

            let dx = to.x - from.x;
            let dy = to.y - from.y;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance <= f32::EPSILON {
                continue;
            }

            let max_offset = (distance * 0.5) - EDGE_COLLISION_MARGIN;
            if max_offset <= 0.0 {
                continue;
            }

            let base_offset = (distance * 0.25)
                .min(EDGE_BIDIRECTIONAL_OFFSET)
                .min(max_offset);
            if base_offset <= 0.0 {
                continue;
            }

            let max_stub = (distance * 0.5) - EDGE_COLLISION_MARGIN;
            if max_stub <= 0.0 {
                continue;
            }

            let stub_base = (distance * 0.25).min(EDGE_BIDIRECTIONAL_STUB).min(max_stub);
            if stub_base <= 0.0 {
                continue;
            }

            let mut first_pair_resolved = false;
            if let (Some(&f_idx0), Some(&b_idx0)) = (forward.first(), backward.first()) {
                if !has_override(f_idx0) && !has_override(b_idx0) {
                    if let Some((forward_points, backward_points)) = self
                        .resolve_bidirectional_pair(
                            from,
                            to,
                            &self.edges[f_idx0],
                            &self.edges[b_idx0],
                        )
                    {
                        auto_points.insert(f_idx0, forward_points.clone());
                        auto_points.insert(b_idx0, backward_points.clone());
                        first_pair_resolved = true;
                    }
                }
            }

            for (i, &edge_idx) in forward.iter().enumerate() {
                if first_pair_resolved && i == 0 {
                    continue;
                }
                if has_override(edge_idx) {
                    continue;
                }

                let factor = 1.0 + i as f32;
                let offset = (base_offset * factor).min(max_offset).max(base_offset);
                let stub = stub_base.min(max_stub);
                auto_points.insert(
                    edge_idx,
                    Self::generate_bidir_points(from, to, offset, stub, 1.0),
                );
            }

            for (i, &edge_idx) in backward.iter().enumerate() {
                if first_pair_resolved && i == 0 {
                    continue;
                }
                if has_override(edge_idx) {
                    continue;
                }

                let factor = 1.0 + i as f32;
                let offset = (base_offset * factor).min(max_offset).max(base_offset);
                let stub = stub_base.min(max_stub);
                let mut points = Self::generate_bidir_points(from, to, offset, stub, -1.0);
                points.reverse();
                auto_points.insert(edge_idx, points);
            }
        }

        for (edge_idx, edge) in self.edges.iter().enumerate() {
            let edge_id = &edge_ids[edge_idx];
            let from = *positions
                .get(&edge.from)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", edge.from))?;
            let to = *positions
                .get(&edge.to)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", edge.to))?;

            let mut middle_points: Vec<Point> = Vec::new();
            let has_custom_override =
                if let Some(custom) = overrides.and_then(|ov| ov.edges.get(edge_id)) {
                    middle_points.extend(custom.points.iter().copied());
                    true
                } else {
                    if let Some(points) = auto_points.get(&edge_idx) {
                        middle_points.extend(points.iter().copied());
                    }
                    false
                };

            let mut path = build_route(from, &middle_points, to);

            let base_label_collision = self.label_collides_with_nodes(edge, &path, &node_bounds);
            let base_node_collision = self.route_collides_with_nodes(edge, &path, &node_bounds);
            let base_intersections = count_route_intersections(&path, &routes);

            if middle_points.is_empty()
                && !has_override(edge_idx)
                && (base_label_collision || base_node_collision || base_intersections > 0)
            {
                if let Some(adjusted) = self.adjust_edge_for_conflicts(
                    from,
                    to,
                    edge,
                    &node_bounds,
                    &routes,
                    base_label_collision,
                    base_node_collision,
                    base_intersections,
                ) {
                    path = build_route(from, &adjusted, to);
                }
            }

            if !has_custom_override {
                let mut detour_attempts = 0_usize;
                while self.route_collides_with_nodes(edge, &path, &node_bounds) {
                    if let Some(candidate) =
                        self.detour_route_for_collisions(edge, &path, &node_bounds, &routes)
                    {
                        path = candidate;
                        detour_attempts += 1;
                        if detour_attempts >= 3 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            if has_custom_override {
                if let Some(custom) = overrides.and_then(|ov| ov.edges.get(edge_id)) {
                    path = build_route(from, &custom.points, to);
                }
            }

            if let (Some(from_bounds), Some(to_bounds)) =
                (node_bounds.get(&edge.from), node_bounds.get(&edge.to))
            {
                trim_route_endpoints(&mut path, from_bounds, to_bounds);
            }

            routes.insert(edge_id.clone(), path);
        }

        Ok(routes)
    }

    fn resolve_bidirectional_pair(
        &self,
        from: Point,
        to: Point,
        forward_edge: &Edge,
        backward_edge: &Edge,
    ) -> Option<(Vec<Point>, Vec<Point>)> {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= f32::EPSILON {
            return None;
        }

        let max_offset = (distance * 0.5) - EDGE_COLLISION_MARGIN;
        if max_offset <= 0.0 {
            return None;
        }

        let max_stub = (distance * 0.5) - EDGE_COLLISION_MARGIN;
        if max_stub <= 0.0 {
            return None;
        }

        let base_offset = (distance * 0.25)
            .min(EDGE_BIDIRECTIONAL_OFFSET)
            .min(max_offset);
        let base_stub = (distance * 0.25).min(EDGE_BIDIRECTIONAL_STUB).min(max_stub);

        if base_offset <= 0.0 || base_stub <= 0.0 {
            return None;
        }

        let from_rect = node_rect(from).inflate(EDGE_COLLISION_MARGIN);
        let to_rect = node_rect(to).inflate(EDGE_COLLISION_MARGIN);

        let mut fallback: Option<(Vec<Point>, Vec<Point>)> = None;

        for attempt in 0..=EDGE_COLLISION_MAX_ITER {
            let offset = (base_offset + attempt as f32 * EDGE_BIDIRECTIONAL_OFFSET_STEP)
                .min(max_offset)
                .max(base_offset);
            let stub = (base_stub + attempt as f32 * EDGE_BIDIRECTIONAL_STUB_STEP)
                .min(max_stub)
                .max(base_stub);

            let forward_points = Diagram::generate_bidir_points(from, to, offset, stub, 1.0);
            let mut backward_points = Diagram::generate_bidir_points(from, to, offset, stub, -1.0);
            backward_points.reverse();

            let forward_route = build_route(from, &forward_points, to);
            let backward_route = build_route(to, &backward_points, from);

            let forward_label = label_rect_for_route(forward_edge, &forward_route)
                .map(|rect| rect.inflate(EDGE_COLLISION_MARGIN));
            let backward_label = label_rect_for_route(backward_edge, &backward_route)
                .map(|rect| rect.inflate(EDGE_COLLISION_MARGIN));

            let mut collision = false;

            if let Some(rect) = forward_label {
                if rect.intersects(&from_rect) || rect.intersects(&to_rect) {
                    collision = true;
                }
            }

            if let Some(rect) = backward_label {
                if rect.intersects(&from_rect) || rect.intersects(&to_rect) {
                    collision = true;
                }
            }

            if let (Some(a), Some(b)) = (forward_label, backward_label) {
                if a.intersects(&b) {
                    collision = true;
                }
            }

            if !collision {
                return Some((forward_points, backward_points));
            }

            fallback = Some((forward_points, backward_points));

            if (offset - max_offset).abs() < f32::EPSILON && (stub - max_stub).abs() < f32::EPSILON
            {
                break;
            }
        }

        fallback
    }

    fn adjust_edge_for_conflicts(
        &self,
        from: Point,
        to: Point,
        edge: &Edge,
        node_bounds: &HashMap<String, NodeBoundary>,
        existing_routes: &HashMap<String, Vec<Point>>,
        base_label_collision: bool,
        base_node_collision: bool,
        base_intersections: usize,
    ) -> Option<Vec<Point>> {
        let base_metric = (
            base_node_collision as u8,
            base_label_collision as u8,
            base_intersections,
        );
        if base_metric == (0_u8, 0_u8, 0_usize) {
            return None;
        }

        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= f32::EPSILON {
            return None;
        }

        let max_offset = (distance * 0.5) - EDGE_COLLISION_MARGIN;
        let max_stub = (distance * 0.5) - EDGE_COLLISION_MARGIN;
        if max_offset <= 0.0 || max_stub <= 0.0 {
            return None;
        }

        let mut base_offset = (distance * 0.25).min(max_offset);
        let mut base_stub = (distance * 0.25).min(max_stub);

        if !base_node_collision {
            base_offset = base_offset.min(EDGE_SINGLE_OFFSET);
            base_stub = base_stub.min(EDGE_SINGLE_STUB);
        }

        if base_offset <= 0.0 || base_stub <= 0.0 {
            return None;
        }

        let mut best_metric = base_metric;
        let mut best_points: Option<Vec<Point>> = None;
        let mut found_perfect = false;

        'search: for &normal_sign in &[1.0, -1.0] {
            for attempt in 0..=EDGE_COLLISION_MAX_ITER {
                let offset = (base_offset + attempt as f32 * EDGE_SINGLE_OFFSET_STEP)
                    .min(max_offset)
                    .max(base_offset);
                let stub = (base_stub + attempt as f32 * EDGE_SINGLE_STUB_STEP)
                    .min(max_stub)
                    .max(base_stub);

                let points = Diagram::generate_bidir_points(from, to, offset, stub, normal_sign);
                if evaluate_candidate_route(
                    self,
                    edge,
                    from,
                    to,
                    node_bounds,
                    existing_routes,
                    points,
                    &mut best_metric,
                    &mut best_points,
                ) {
                    found_perfect = true;
                    break 'search;
                }

                if (offset - max_offset).abs() < f32::EPSILON
                    && (stub - max_stub).abs() < f32::EPSILON
                {
                    break;
                }
            }
        }

        if found_perfect {
            return best_points;
        }

        for candidate in generate_axis_detours(from, to) {
            if evaluate_candidate_route(
                self,
                edge,
                from,
                to,
                node_bounds,
                existing_routes,
                candidate,
                &mut best_metric,
                &mut best_points,
            ) {
                found_perfect = true;
                break;
            }
        }

        if found_perfect {
            return best_points;
        }

        if best_metric < base_metric {
            best_points
        } else {
            None
        }
    }

    fn detour_route_for_collisions(
        &self,
        edge: &Edge,
        route: &[Point],
        node_bounds: &HashMap<String, NodeBoundary>,
        existing_routes: &HashMap<String, Vec<Point>>,
    ) -> Option<Vec<Point>> {
        if route.len() < 2 {
            return None;
        }

        let mut best_metric = (
            self.route_collides_with_nodes(edge, route, node_bounds) as u8,
            self.label_collides_with_nodes(edge, route, node_bounds) as u8,
            count_route_intersections(route, existing_routes),
        );

        if best_metric.0 == 0 {
            return None;
        }

        let clearance = EDGE_COLLISION_MARGIN * 2.0 + 8.0;
        let mut best_route: Option<Vec<Point>> = None;

        for segment_idx in 0..route.len() - 1 {
            let a = route[segment_idx];
            let b = route[segment_idx + 1];

            for (node_id, bounds) in node_bounds {
                if node_id == &edge.from || node_id == &edge.to {
                    continue;
                }

                let inflated = bounds.rect.inflate(EDGE_COLLISION_MARGIN);
                if !inflated.intersects_segment(a, b) {
                    continue;
                }

                let detour_candidates = [
                    vec![
                        Point {
                            x: a.x,
                            y: inflated.min_y - clearance,
                        },
                        Point {
                            x: b.x,
                            y: inflated.min_y - clearance,
                        },
                    ],
                    vec![
                        Point {
                            x: a.x,
                            y: inflated.max_y + clearance,
                        },
                        Point {
                            x: b.x,
                            y: inflated.max_y + clearance,
                        },
                    ],
                    vec![
                        Point {
                            x: inflated.min_x - clearance,
                            y: a.y,
                        },
                        Point {
                            x: inflated.min_x - clearance,
                            y: b.y,
                        },
                    ],
                    vec![
                        Point {
                            x: inflated.max_x + clearance,
                            y: a.y,
                        },
                        Point {
                            x: inflated.max_x + clearance,
                            y: b.y,
                        },
                    ],
                ];

                for detour in detour_candidates {
                    let mut candidate = Vec::new();
                    candidate.extend_from_slice(&route[..=segment_idx]);
                    candidate.extend(detour.iter());
                    candidate.extend_from_slice(&route[segment_idx + 1..]);
                    simplify_route(&mut candidate);

                    let candidate_metric = (
                        self.route_collides_with_nodes(edge, &candidate, node_bounds) as u8,
                        self.label_collides_with_nodes(edge, &candidate, node_bounds) as u8,
                        count_route_intersections(&candidate, existing_routes),
                    );

                    if candidate_metric < best_metric {
                        best_metric = candidate_metric;
                        best_route = Some(candidate);
                        if best_metric == (0, 0, 0) {
                            return best_route;
                        }
                    }
                }
            }
        }

        best_route
    }

    fn label_collides_with_nodes(
        &self,
        edge: &Edge,
        route: &[Point],
        node_bounds: &HashMap<String, NodeBoundary>,
    ) -> bool {
        let rect = match label_rect_for_route(edge, route) {
            Some(rect) => rect.inflate(EDGE_COLLISION_MARGIN),
            None => return false,
        };

        node_bounds
            .values()
            .any(|bounds| rect.intersects(&bounds.rect))
    }

    fn route_collides_with_nodes(
        &self,
        edge: &Edge,
        route: &[Point],
        node_bounds: &HashMap<String, NodeBoundary>,
    ) -> bool {
        if route.len() < 2 {
            return false;
        }

        for segment in route.windows(2) {
            let a = segment[0];
            let b = segment[1];
            for (node_id, bounds) in node_bounds {
                if node_id == &edge.from || node_id == &edge.to {
                    continue;
                }
                if bounds
                    .rect
                    .inflate(EDGE_COLLISION_MARGIN)
                    .intersects_segment(a, b)
                {
                    return true;
                }
            }
        }

        false
    }

    fn generate_bidir_points(
        from: Point,
        to: Point,
        offset: f32,
        stub: f32,
        normal_sign: f32,
    ) -> Vec<Point> {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= f32::EPSILON {
            return Vec::new();
        }

        let tangent_x = dx / distance;
        let tangent_y = dy / distance;
        let normal_x = -tangent_y;
        let normal_y = tangent_x;

        let offset_vec_x = normal_x * offset * normal_sign;
        let offset_vec_y = normal_y * offset * normal_sign;

        let stub_clamped = stub.min(distance / 2.0 - 1.0).max(0.0);
        if stub_clamped <= 0.0 {
            return vec![Point {
                x: (from.x + to.x) * 0.5 + offset_vec_x,
                y: (from.y + to.y) * 0.5 + offset_vec_y,
            }];
        }

        let stub_vec_x = tangent_x * stub_clamped;
        let stub_vec_y = tangent_y * stub_clamped;

        let first = Point {
            x: from.x + stub_vec_x + offset_vec_x,
            y: from.y + stub_vec_y + offset_vec_y,
        };

        let middle = Point {
            x: (from.x + to.x) * 0.5 + offset_vec_x,
            y: (from.y + to.y) * 0.5 + offset_vec_y,
        };

        let second = Point {
            x: to.x - stub_vec_x + offset_vec_x,
            y: to.y - stub_vec_y + offset_vec_y,
        };

        vec![first, middle, second]
    }

    pub fn remove_node(&mut self, node_id: &str) -> bool {
        let existed = self.nodes.remove(node_id).is_some();
        if existed {
            self.order.retain(|id| id != node_id);
            self.edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
            self.node_membership.remove(node_id);
            prune_node_from_subgraphs(&mut self.subgraphs, node_id);
        }
        existed
    }

    pub fn remove_edge_by_identifier(&mut self, edge_id: &str) -> bool {
        let before = self.edges.len();
        self.edges.retain(|edge| edge_identifier(edge) != edge_id);
        before != self.edges.len()
    }

    pub fn to_definition(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("graph {}", self.direction.as_token()));

        let mut emitted = HashSet::new();

        for (idx, subgraph) in self.subgraphs.iter().enumerate() {
            self.emit_subgraph_definition(subgraph, 1, &mut lines, &mut emitted);
            if idx + 1 != self.subgraphs.len() {
                lines.push(String::new());
            }
        }

        if !self.subgraphs.is_empty() && self.order.iter().any(|id| !emitted.contains(id)) {
            lines.push(String::new());
        }

        for id in &self.order {
            if emitted.contains(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(id) {
                lines.push(Self::format_node_line(id, node));
            }
        }

        if !self.edges.is_empty() && !lines.is_empty() {
            lines.push(String::new());
        }

        for edge in &self.edges {
            lines.push(Self::format_edge_line(edge));
        }

        while matches!(lines.last(), Some(line) if line.is_empty()) {
            lines.pop();
        }

        let mut output = lines.join("\n");
        output.push('\n');
        output
    }

    fn emit_subgraph_definition(
        &self,
        subgraph: &Subgraph,
        depth: usize,
        lines: &mut Vec<String>,
        emitted: &mut HashSet<String>,
    ) {
        let indent = "    ".repeat(depth);
        let header = if subgraph.label == subgraph.id {
            subgraph.id.clone()
        } else {
            format!("{}[{}]", subgraph.id, subgraph.label)
        };
        lines.push(format!("{}subgraph {}", indent, header));

        let inner_indent = "    ".repeat(depth + 1);
        let direct_nodes: HashSet<&str> = subgraph.nodes.iter().map(|id| id.as_str()).collect();
        for id in &self.order {
            if !direct_nodes.contains(id.as_str()) {
                continue;
            }
            if emitted.insert(id.clone()) {
                if let Some(node) = self.nodes.get(id) {
                    lines.push(format!(
                        "{}{}",
                        inner_indent,
                        Self::format_node_line(id, node)
                    ));
                }
            }
        }

        for child in &subgraph.children {
            self.emit_subgraph_definition(child, depth + 1, lines, emitted);
        }

        lines.push(format!("{}end", indent));
    }

    fn format_node_line(id: &str, node: &Node) -> String {
        node.shape.format_spec(id, &node.label)
    }

    fn format_edge_line(edge: &Edge) -> String {
        if let Some(label) = &edge.label {
            format!(
                "{} {}|{}| {}",
                edge.from,
                edge.kind.arrow_token(),
                label,
                edge.to
            )
        } else {
            format!("{} {} {}", edge.from, edge.kind.arrow_token(), edge.to)
        }
    }
}

fn evaluate_candidate_route(
    diagram: &Diagram,
    edge: &Edge,
    from: Point,
    to: Point,
    node_bounds: &HashMap<String, NodeBoundary>,
    existing_routes: &HashMap<String, Vec<Point>>,
    points: Vec<Point>,
    best_metric: &mut (u8, u8, usize),
    best_points: &mut Option<Vec<Point>>,
) -> bool {
    let route = build_route(from, &points, to);
    let node_collision = diagram.route_collides_with_nodes(edge, &route, node_bounds);
    let label_collision = diagram.label_collides_with_nodes(edge, &route, node_bounds);
    let intersections = count_route_intersections(&route, existing_routes);
    let candidate_metric = (node_collision as u8, label_collision as u8, intersections);

    if candidate_metric < *best_metric {
        *best_metric = candidate_metric;
        *best_points = Some(points);
    }

    *best_metric == (0_u8, 0_u8, 0_usize)
}

fn generate_axis_detours(from: Point, to: Point) -> Vec<Vec<Point>> {
    let mut candidates = Vec::new();

    let horizontal_span = (from.x - to.x).abs();
    let vertical_span = (from.y - to.y).abs();

    let vertical_clearance = NODE_HEIGHT + EDGE_COLLISION_MARGIN * 4.0;
    let horizontal_clearance = NODE_WIDTH + EDGE_COLLISION_MARGIN * 4.0;

    if horizontal_span > NODE_WIDTH * 0.5 {
        let above = from.y.min(to.y) - vertical_clearance;
        candidates.push(vec![
            Point {
                x: from.x,
                y: above,
            },
            Point { x: to.x, y: above },
        ]);

        let below = from.y.max(to.y) + vertical_clearance;
        candidates.push(vec![
            Point {
                x: from.x,
                y: below,
            },
            Point { x: to.x, y: below },
        ]);
    }

    if vertical_span > NODE_HEIGHT * 0.5 {
        let left = from.x.min(to.x) - horizontal_clearance;
        candidates.push(vec![
            Point { x: left, y: from.y },
            Point { x: left, y: to.y },
        ]);

        let right = from.x.max(to.x) + horizontal_clearance;
        candidates.push(vec![
            Point {
                x: right,
                y: from.y,
            },
            Point { x: right, y: to.y },
        ]);
    }

    candidates
}

impl NodeShape {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeShape::Rectangle => "rectangle",
            NodeShape::Stadium => "stadium",
            NodeShape::Circle => "circle",
            NodeShape::Diamond => "diamond",
        }
    }

    fn default_fill_color(&self) -> &'static str {
        match self {
            NodeShape::Rectangle => "#fde68a",
            NodeShape::Stadium => "#c4f1f9",
            NodeShape::Circle => "#e9d8fd",
            NodeShape::Diamond => "#fbcfe8",
        }
    }

    fn format_spec(&self, id: &str, label: &str) -> String {
        match self {
            NodeShape::Rectangle => {
                if label == id {
                    id.to_string()
                } else {
                    format!("{id}[{label}]")
                }
            }
            NodeShape::Stadium => format!("{id}({label})"),
            NodeShape::Circle => format!("{id}(({label}))"),
            NodeShape::Diamond => format!("{id}{{{label}}}"),
        }
    }
}

impl Direction {
    fn as_token(&self) -> &'static str {
        match self {
            Direction::TopDown => "TD",
            Direction::LeftRight => "LR",
            Direction::BottomTop => "BT",
            Direction::RightLeft => "RL",
        }
    }
}

impl EdgeKind {
    pub fn arrow_token(&self) -> &'static str {
        match self {
            EdgeKind::Solid => "-->",
            EdgeKind::Dashed => "-.->",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Solid => "solid",
            EdgeKind::Dashed => "dashed",
        }
    }
}

fn normalize_label_lines(label: &str) -> Vec<String> {
    label
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                " ".to_string()
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn measure_label_box(lines: &[String]) -> (f32, f32) {
    let mut max_chars = 0_usize;
    for line in lines {
        max_chars = max_chars.max(line.chars().count());
    }

    let width = (EDGE_LABEL_CHAR_WIDTH * max_chars as f32 + EDGE_LABEL_HORIZONTAL_PADDING)
        .max(EDGE_LABEL_MIN_WIDTH);
    let height = (EDGE_LABEL_LINE_HEIGHT * lines.len() as f32 + EDGE_LABEL_VERTICAL_PADDING)
        .max(EDGE_LABEL_MIN_HEIGHT);

    (width, height)
}

fn label_center_for_route(route: &[Point]) -> Point {
    if route.is_empty() {
        return Point {
            x: 0.0,
            y: -EDGE_LABEL_VERTICAL_OFFSET,
        };
    }

    let fallback = centroid(route);
    if route.len() <= 2 {
        return Point {
            x: fallback.x,
            y: fallback.y - EDGE_LABEL_VERTICAL_OFFSET,
        };
    }

    let handle_points = &route[1..route.len() - 1];
    if handle_points.is_empty() {
        return Point {
            x: fallback.x,
            y: fallback.y - EDGE_LABEL_VERTICAL_OFFSET,
        };
    }

    if handle_points.len() == 1 {
        return handle_points[0];
    }

    let mut best = handle_points[0];
    let mut best_distance = f32::INFINITY;
    for point in handle_points.iter().copied() {
        let dx = point.x - fallback.x;
        let dy = point.y - fallback.y;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance < best_distance {
            best_distance = distance;
            best = point;
        }
    }

    best
}

fn build_route(start: Point, middle: &[Point], end: Point) -> Vec<Point> {
    let mut route = Vec::with_capacity(middle.len() + 2);
    route.push(start);
    route.extend_from_slice(middle);
    route.push(end);
    route
}

fn simplify_route(route: &mut Vec<Point>) {
    if route.is_empty() {
        return;
    }

    route.dedup_by(|a, b| points_close(*a, *b));

    if route.len() < 3 {
        return;
    }

    let mut idx = 1;
    while idx + 1 < route.len() {
        let prev = route[idx - 1];
        let current = route[idx];
        let next = route[idx + 1];

        if orientation(prev, current, next).abs() < 1e-3_f32 {
            let within_x = current.x >= prev.x.min(next.x) - 1e-3_f32
                && current.x <= prev.x.max(next.x) + 1e-3_f32;
            let within_y = current.y >= prev.y.min(next.y) - 1e-3_f32
                && current.y <= prev.y.max(next.y) + 1e-3_f32;
            if within_x && within_y {
                route.remove(idx);
                continue;
            }
        }

        idx += 1;
    }
}

fn label_rect_for_route(edge: &Edge, route: &[Point]) -> Option<Rect> {
    let label = edge.label.as_ref()?;
    let lines = normalize_label_lines(label);
    if lines.is_empty() {
        return None;
    }

    let (box_width, box_height) = measure_label_box(&lines);
    let center = label_center_for_route(route);

    Some(Rect {
        min_x: center.x - box_width / 2.0,
        max_x: center.x + box_width / 2.0,
        min_y: center.y - box_height / 2.0,
        max_y: center.y + box_height / 2.0,
    })
}

fn node_rect(center: Point) -> Rect {
    Rect {
        min_x: center.x - NODE_WIDTH / 2.0,
        max_x: center.x + NODE_WIDTH / 2.0,
        min_y: center.y - NODE_HEIGHT / 2.0,
        max_y: center.y + NODE_HEIGHT / 2.0,
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl Rect {
    fn inflate(self, amount: f32) -> Rect {
        Rect {
            min_x: self.min_x - amount,
            max_x: self.max_x + amount,
            min_y: self.min_y - amount,
            max_y: self.max_y + amount,
        }
    }

    fn intersects(&self, other: &Rect) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    fn contains(&self, point: Point) -> bool {
        let eps = 1e-3_f32;
        point.x >= self.min_x - eps
            && point.x <= self.max_x + eps
            && point.y >= self.min_y - eps
            && point.y <= self.max_y + eps
    }

    fn intersects_segment(&self, a: Point, b: Point) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }

        let top_left = Point {
            x: self.min_x,
            y: self.min_y,
        };
        let top_right = Point {
            x: self.max_x,
            y: self.min_y,
        };
        let bottom_right = Point {
            x: self.max_x,
            y: self.max_y,
        };
        let bottom_left = Point {
            x: self.min_x,
            y: self.max_y,
        };

        let edges = [
            (top_left, top_right),
            (top_right, bottom_right),
            (bottom_right, bottom_left),
            (bottom_left, top_left),
        ];

        edges
            .iter()
            .any(|(p1, p2)| segments_intersect(a, b, *p1, *p2))
    }
}

#[derive(Clone, Copy, Debug)]
struct NodeBoundary {
    center: Point,
    shape: NodeShape,
    rect: Rect,
}

impl NodeBoundary {
    fn new(center: Point, shape: NodeShape) -> Self {
        Self {
            center,
            shape,
            rect: node_rect(center),
        }
    }

    fn contains_point(&self, point: Point) -> bool {
        match self.shape {
            NodeShape::Rectangle | NodeShape::Stadium => self.rect.contains(point),
            NodeShape::Circle => {
                let rx = NODE_WIDTH / 2.0;
                let ry = NODE_HEIGHT / 2.0;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let norm_x = (point.x - self.center.x) / rx;
                let norm_y = (point.y - self.center.y) / ry;
                norm_x * norm_x + norm_y * norm_y <= 1.0 + 1e-3_f32
            }
            NodeShape::Diamond => {
                let half_w = NODE_WIDTH / 2.0;
                let half_h = NODE_HEIGHT / 2.0;
                if half_w <= 0.0 || half_h <= 0.0 {
                    return false;
                }
                let dx = (point.x - self.center.x).abs() / half_w;
                let dy = (point.y - self.center.y).abs() / half_h;
                dx + dy <= 1.0 + 1e-3_f32
            }
        }
    }
}

fn trim_route_endpoints(
    path: &mut Vec<Point>,
    from_bounds: &NodeBoundary,
    to_bounds: &NodeBoundary,
) {
    if path.len() < 2 {
        return;
    }

    if from_bounds.contains_point(path[0]) {
        if let Some(trimmed) = clip_segment_exit_with_shape(path[0], path[1], from_bounds, false) {
            path[0] = trimmed;
        }
    }

    if path.len() < 2 {
        return;
    }

    let last = path.len() - 1;
    if to_bounds.contains_point(path[last]) {
        if let Some(trimmed) =
            clip_segment_exit_with_shape(path[last], path[last - 1], to_bounds, true)
        {
            path[last] = trimmed;
        }
    }
}

fn clip_segment_exit_with_shape(
    start: Point,
    next: Point,
    bounds: &NodeBoundary,
    extend_outward: bool,
) -> Option<Point> {
    match bounds.shape {
        NodeShape::Rectangle | NodeShape::Stadium => {
            clip_segment_exit_rect(start, next, bounds.rect, extend_outward)
        }
        NodeShape::Circle => clip_segment_exit_circle(start, next, bounds, extend_outward),
        NodeShape::Diamond => clip_segment_exit_diamond(start, next, bounds, extend_outward),
    }
}

fn clip_segment_exit_rect(
    start: Point,
    next: Point,
    rect: Rect,
    extend_outward: bool,
) -> Option<Point> {
    let dx = next.x - start.x;
    let dy = next.y - start.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= f32::EPSILON {
        return None;
    }

    let mut candidates = Vec::new();
    if dx.abs() > f32::EPSILON {
        let target_x = if dx > 0.0 { rect.max_x } else { rect.min_x };
        let t = (target_x - start.x) / dx;
        if t >= 0.0 && t <= 1.0 {
            let y = start.y + t * dy;
            if y >= rect.min_y - 1e-3_f32 && y <= rect.max_y + 1e-3_f32 {
                candidates.push(t);
            }
        }
    }
    if dy.abs() > f32::EPSILON {
        let target_y = if dy > 0.0 { rect.max_y } else { rect.min_y };
        let t = (target_y - start.y) / dy;
        if t >= 0.0 && t <= 1.0 {
            let x = start.x + t * dx;
            if x >= rect.min_x - 1e-3_f32 && x <= rect.max_x + 1e-3_f32 {
                candidates.push(t);
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    let mut t_exit = candidates
        .into_iter()
        .fold(1.0_f32, |acc, t| acc.min(t.max(f32::EPSILON)));
    t_exit = t_exit.clamp(0.0, 1.0);

    let mut point = Point {
        x: start.x + t_exit * dx,
        y: start.y + t_exit * dy,
    };

    if extend_outward {
        let dir_x = dx / distance;
        let dir_y = dy / distance;
        point.x += dir_x * EDGE_ARROW_EXTENSION;
        point.y += dir_y * EDGE_ARROW_EXTENSION;
    }

    Some(point)
}

fn clip_segment_exit_circle(
    start: Point,
    next: Point,
    bounds: &NodeBoundary,
    extend_outward: bool,
) -> Option<Point> {
    let dx = next.x - start.x;
    let dy = next.y - start.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= f32::EPSILON {
        return None;
    }

    let rx = NODE_WIDTH / 2.0;
    let ry = NODE_HEIGHT / 2.0;
    let sx = start.x - bounds.center.x;
    let sy = start.y - bounds.center.y;

    let a = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry);
    if a.abs() <= f32::EPSILON {
        return None;
    }

    let b = 2.0 * ((sx * dx) / (rx * rx) + (sy * dy) / (ry * ry));
    let c = (sx * sx) / (rx * rx) + (sy * sy) / (ry * ry) - 1.0;

    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }

    let sqrt_disc = discriminant.sqrt();
    let mut candidates = Vec::new();
    let t0 = (-b + sqrt_disc) / (2.0 * a);
    let t1 = (-b - sqrt_disc) / (2.0 * a);
    if t0 >= 0.0 && t0 <= 1.0 {
        candidates.push(t0);
    }
    if t1 >= 0.0 && t1 <= 1.0 {
        candidates.push(t1);
    }

    if candidates.is_empty() {
        return None;
    }

    let mut t_exit = candidates
        .into_iter()
        .fold(1.0_f32, |acc, t| acc.min(t.max(f32::EPSILON)));
    t_exit = t_exit.clamp(0.0, 1.0);

    let mut point = Point {
        x: start.x + t_exit * dx,
        y: start.y + t_exit * dy,
    };

    if extend_outward {
        let dir_x = dx / distance;
        let dir_y = dy / distance;
        point.x += dir_x * EDGE_ARROW_EXTENSION;
        point.y += dir_y * EDGE_ARROW_EXTENSION;
    }

    Some(point)
}

fn clip_segment_exit_diamond(
    start: Point,
    next: Point,
    bounds: &NodeBoundary,
    extend_outward: bool,
) -> Option<Point> {
    let dx = next.x - start.x;
    let dy = next.y - start.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= f32::EPSILON {
        return None;
    }

    let half_w = NODE_WIDTH / 2.0;
    let half_h = NODE_HEIGHT / 2.0;
    let top = Point {
        x: bounds.center.x,
        y: bounds.center.y - half_h,
    };
    let right = Point {
        x: bounds.center.x + half_w,
        y: bounds.center.y,
    };
    let bottom = Point {
        x: bounds.center.x,
        y: bounds.center.y + half_h,
    };
    let left = Point {
        x: bounds.center.x - half_w,
        y: bounds.center.y,
    };

    let edges = [(top, right), (right, bottom), (bottom, left), (left, top)];

    let mut best_t: Option<f32> = None;
    for (edge_start, edge_end) in edges {
        if let Some(t) = segment_intersection_param(start, next, edge_start, edge_end) {
            if t >= 0.0 && t <= 1.0 {
                let t = t.max(f32::EPSILON);
                best_t = Some(best_t.map_or(t, |current| current.min(t)));
            }
        }
    }

    let t_exit = match best_t {
        Some(t) => t.clamp(0.0, 1.0),
        None => return None,
    };

    let mut point = Point {
        x: start.x + t_exit * dx,
        y: start.y + t_exit * dy,
    };

    if extend_outward {
        let dir_x = dx / distance;
        let dir_y = dy / distance;
        point.x += dir_x * EDGE_ARROW_EXTENSION;
        point.y += dir_y * EDGE_ARROW_EXTENSION;
    }

    Some(point)
}

fn segment_intersection_param(
    start: Point,
    next: Point,
    edge_start: Point,
    edge_end: Point,
) -> Option<f32> {
    let r = Point {
        x: next.x - start.x,
        y: next.y - start.y,
    };
    let s = Point {
        x: edge_end.x - edge_start.x,
        y: edge_end.y - edge_start.y,
    };

    let denom = r.x * s.y - r.y * s.x;
    if denom.abs() < 1e-6_f32 {
        return None;
    }

    let qp = Point {
        x: edge_start.x - start.x,
        y: edge_start.y - start.y,
    };

    let t = (qp.x * s.y - qp.y * s.x) / denom;
    let u = (qp.x * r.y - qp.y * r.x) / denom;

    if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 {
        Some(t)
    } else {
        None
    }
}

fn count_route_intersections(
    route: &[Point],
    existing_routes: &HashMap<String, Vec<Point>>,
) -> usize {
    existing_routes
        .values()
        .filter(|other| routes_intersect(route, other))
        .count()
}

fn routes_intersect(a: &[Point], b: &[Point]) -> bool {
    for segment_a in a.windows(2) {
        for segment_b in b.windows(2) {
            if shares_endpoint(segment_a[0], segment_a[1], segment_b[0], segment_b[1]) {
                continue;
            }
            if segments_intersect(segment_a[0], segment_a[1], segment_b[0], segment_b[1]) {
                return true;
            }
        }
    }
    false
}

fn shares_endpoint(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    points_close(a1, b1) || points_close(a1, b2) || points_close(a2, b1) || points_close(a2, b2)
}

fn segments_intersect(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let o1 = orientation(a1, a2, b1);
    let o2 = orientation(a1, a2, b2);
    let o3 = orientation(b1, b2, a1);
    let o4 = orientation(b1, b2, a2);

    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }

    if o1.abs() < 1e-3_f32 && on_segment(a1, a2, b1) {
        return true;
    }
    if o2.abs() < 1e-3_f32 && on_segment(a1, a2, b2) {
        return true;
    }
    if o3.abs() < 1e-3_f32 && on_segment(b1, b2, a1) {
        return true;
    }
    if o4.abs() < 1e-3_f32 && on_segment(b1, b2, a2) {
        return true;
    }

    false
}

fn orientation(a: Point, b: Point, c: Point) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_segment(a: Point, b: Point, c: Point) -> bool {
    let eps = 1e-3_f32;
    c.x >= a.x.min(b.x) - eps
        && c.x <= a.x.max(b.x) + eps
        && c.y >= a.y.min(b.y) - eps
        && c.y <= a.y.max(b.y) + eps
}

fn points_close(a: Point, b: Point) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt() < 1e-2_f32
}

pub fn align_geometry(
    positions: &HashMap<String, Point>,
    routes: &HashMap<String, Vec<Point>>,
    edges: &[Edge],
    subgraphs: &[Subgraph],
) -> Result<Geometry> {
    if positions.is_empty() {
        bail!("diagram does not declare any nodes");
    }

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for point in positions.values() {
        min_x = min_x.min(point.x - NODE_WIDTH / 2.0);
        max_x = max_x.max(point.x + NODE_WIDTH / 2.0);
        min_y = min_y.min(point.y - NODE_HEIGHT / 2.0);
        max_y = max_y.max(point.y + NODE_HEIGHT / 2.0);
    }

    for path in routes.values() {
        for point in path {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
    }

    for edge in edges {
        if let Some(label) = &edge.label {
            let identifier = edge_identifier(edge);
            let route = routes
                .get(&identifier)
                .ok_or_else(|| anyhow!("missing geometry for edge '{identifier}'"))?;

            let lines = normalize_label_lines(label);
            if lines.is_empty() {
                continue;
            }

            let (box_width, box_height) = measure_label_box(&lines);
            let center = label_center_for_route(route);
            let half_w = box_width / 2.0;
            let half_h = box_height / 2.0;

            min_x = min_x.min(center.x - half_w);
            max_x = max_x.max(center.x + half_w);
            min_y = min_y.min(center.y - half_h);
            max_y = max_y.max(center.y + half_h);
        }
    }

    let unshifted_subgraphs = compute_subgraph_visuals(subgraphs, positions);
    for sg in &unshifted_subgraphs {
        min_x = min_x.min(sg.x);
        max_x = max_x.max(sg.x + sg.width);
        min_y = min_y.min(sg.y);
        max_y = max_y.max(sg.y + sg.height);
    }

    if min_x > max_x || min_y > max_y {
        bail!("unable to compute diagram bounds");
    }

    let width = (max_x - min_x).max(NODE_WIDTH) + LAYOUT_MARGIN * 2.0;
    let height = (max_y - min_y).max(NODE_HEIGHT) + LAYOUT_MARGIN * 2.0;

    let shift_x = LAYOUT_MARGIN - min_x;
    let shift_y = LAYOUT_MARGIN - min_y;

    let mut shifted_positions = HashMap::new();
    for (id, point) in positions {
        shifted_positions.insert(
            id.clone(),
            Point {
                x: point.x + shift_x,
                y: point.y + shift_y,
            },
        );
    }

    let mut shifted_routes = HashMap::new();
    for (id, path) in routes {
        let mut shifted = Vec::with_capacity(path.len());
        for point in path {
            shifted.push(Point {
                x: point.x + shift_x,
                y: point.y + shift_y,
            });
        }
        shifted_routes.insert(id.clone(), shifted);
    }

    let shifted_subgraphs = unshifted_subgraphs
        .into_iter()
        .map(|mut sg| {
            sg.x += shift_x;
            sg.y += shift_y;
            sg.label_x += shift_x;
            sg.label_y += shift_y;
            sg
        })
        .collect();

    Ok(Geometry {
        positions: shifted_positions,
        edges: shifted_routes,
        subgraphs: shifted_subgraphs,
        width,
        height,
    })
}

fn compute_subgraph_visuals(
    subgraphs: &[Subgraph],
    positions: &HashMap<String, Point>,
) -> Vec<SubgraphVisual> {
    let mut visuals = Vec::new();
    for subgraph in subgraphs {
        collect_subgraph_visual(subgraph, positions, &mut visuals, 0);
    }

    visuals.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| a.id.cmp(&b.id))
    });
    visuals
}

fn collect_subgraph_visual(
    subgraph: &Subgraph,
    positions: &HashMap<String, Point>,
    visuals: &mut Vec<SubgraphVisual>,
    depth: usize,
) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;

    for child in &subgraph.children {
        if let Some(child_bounds) = collect_subgraph_visual(child, positions, visuals, depth + 1) {
            expand_bounds(&mut bounds, child_bounds);
        }
    }

    for node_id in &subgraph.nodes {
        if let Some(position) = positions.get(node_id) {
            expand_bounds(&mut bounds, node_rect(*position));
        }
    }

    let mut bounds = match bounds {
        Some(bounds) => bounds,
        None => return None,
    };

    bounds.min_x -= SUBGRAPH_PADDING;
    bounds.max_x += SUBGRAPH_PADDING;
    bounds.min_y -= SUBGRAPH_PADDING;
    bounds.max_y += SUBGRAPH_PADDING;

    let mut outer = bounds;
    outer.min_y -= SUBGRAPH_LABEL_AREA;

    let mut width = outer.max_x - outer.min_x;
    let mut height = outer.max_y - outer.min_y;

    let min_width = NODE_WIDTH + SUBGRAPH_PADDING * 2.0;
    if width < min_width {
        let delta = (min_width - width) / 2.0;
        outer.min_x -= delta;
        outer.max_x += delta;
    }

    let min_height = NODE_HEIGHT + SUBGRAPH_PADDING * 2.0 + SUBGRAPH_LABEL_AREA;
    if height < min_height {
        let delta = (min_height - height) / 2.0;
        outer.min_y -= delta;
        outer.max_y += delta;
    }

    width = outer.max_x - outer.min_x;
    height = outer.max_y - outer.min_y;

    let visual = SubgraphVisual {
        id: subgraph.id.clone(),
        label: subgraph.label.clone(),
        x: outer.min_x,
        y: outer.min_y,
        width,
        height,
        label_x: outer.min_x + SUBGRAPH_LABEL_INSET_X,
        label_y: outer.min_y + SUBGRAPH_LABEL_TEXT_BASELINE,
        depth,
        order: subgraph.order,
    };

    visuals.push(visual);

    Some(Rect {
        min_x: outer.min_x,
        max_x: outer.max_x,
        min_y: outer.min_y,
        max_y: outer.max_y,
    })
}

fn expand_bounds(target: &mut Option<Rect>, rect: Rect) {
    if let Some(existing) = target.as_mut() {
        existing.min_x = existing.min_x.min(rect.min_x);
        existing.max_x = existing.max_x.max(rect.max_x);
        existing.min_y = existing.min_y.min(rect.min_y);
        existing.max_y = existing.max_y.max(rect.max_y);
    } else {
        *target = Some(rect);
    }
}

fn compute_canvas_size_for_positions(positions: &HashMap<String, Point>) -> CanvasSize {
    if positions.is_empty() {
        return CanvasSize {
            width: START_OFFSET * 2.0 + NODE_WIDTH,
            height: START_OFFSET * 2.0 + NODE_HEIGHT,
        };
    }

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for point in positions.values() {
        min_x = min_x.min(point.x - NODE_WIDTH / 2.0);
        max_x = max_x.max(point.x + NODE_WIDTH / 2.0);
        min_y = min_y.min(point.y - NODE_HEIGHT / 2.0);
        max_y = max_y.max(point.y + NODE_HEIGHT / 2.0);
    }

    let width = (max_x - min_x).max(NODE_WIDTH) + LAYOUT_MARGIN * 2.0;
    let height = (max_y - min_y).max(NODE_HEIGHT) + LAYOUT_MARGIN * 2.0;

    CanvasSize { width, height }
}

fn gather_subgraph_nodes(subgraph: &Subgraph) -> HashSet<String> {
    let mut nodes = HashSet::new();
    collect_nodes_recursive(subgraph, &mut nodes);
    nodes
}

fn collect_nodes_recursive(subgraph: &Subgraph, nodes: &mut HashSet<String>) {
    for id in &subgraph.nodes {
        nodes.insert(id.clone());
    }
    for child in &subgraph.children {
        collect_nodes_recursive(child, nodes);
    }
}

fn compute_group_bounds(
    nodes: &HashSet<String>,
    positions: &HashMap<String, Point>,
) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for id in nodes {
        if let Some(position) = positions.get(id) {
            expand_bounds(&mut bounds, node_rect(*position));
        }
    }
    bounds
}

fn offset_nodes(positions: &mut HashMap<String, Point>, nodes: &HashSet<String>, dx: f32, dy: f32) {
    for id in nodes {
        if let Some(point) = positions.get_mut(id) {
            point.x += dx;
            point.y += dy;
        }
    }
}

fn rects_intersect_with_margin(a: &Rect, b: &Rect, margin: f32) -> bool {
    (a.min_x - margin) < (b.max_x + margin)
        && (a.max_x + margin) > (b.min_x - margin)
        && (a.min_y - margin) < (b.max_y + margin)
        && (a.max_y + margin) > (b.min_y - margin)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    x: f32,
    y: f32,
}

pub fn centroid(points: &[Point]) -> Point {
    if points.is_empty() {
        return Point { x: 0.0, y: 0.0 };
    }

    let (sum_x, sum_y) = points.iter().fold((0.0_f32, 0.0_f32), |acc, point| {
        (acc.0 + point.x, acc.1 + point.y)
    });
    let count = points.len() as f32;
    Point {
        x: sum_x / count,
        y: sum_y / count,
    }
}

pub fn edge_identifier(edge: &Edge) -> String {
    format!("{} {} {}", edge.from, edge.kind.arrow_token(), edge.to)
}

fn parse_graph_header(line: &str) -> Result<Direction> {
    let mut parts = line.split_whitespace();
    let keyword = parts
        .next()
        .ok_or_else(|| anyhow!("empty header line"))?
        .to_ascii_lowercase();

    if keyword != "graph" {
        bail!("diagram must start with 'graph', found '{keyword}'");
    }

    let direction_token = parts.next().unwrap_or("TD").trim().to_ascii_uppercase();
    let direction = match direction_token.as_str() {
        "TD" | "TB" => Direction::TopDown,
        "BT" => Direction::BottomTop,
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        other => {
            bail!("unsupported direction '{other}' in header; supported values are TD, BT, LR, RL")
        }
    };

    Ok(direction)
}

fn parse_subgraph_header(raw: &str) -> Result<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("subgraph declaration missing identifier");
    }

    if let Some(start) = trimmed.find('[') {
        if trimmed.ends_with(']') && start < trimmed.len() - 1 {
            let id_part = trimmed[..start].trim();
            if id_part.is_empty() {
                bail!("subgraph identifier cannot be empty");
            }
            let label_part = trimmed[start + 1..trimmed.len() - 1].trim();
            let label = if label_part.is_empty() {
                id_part
            } else {
                label_part
            };
            return Ok((normalize_subgraph_id(id_part), label.to_string()));
        }
    }

    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let label = trimmed[1..trimmed.len() - 1].trim();
        if label.is_empty() {
            bail!("subgraph label cannot be empty");
        }
        return Ok((normalize_subgraph_id(label), label.to_string()));
    }

    Ok((normalize_subgraph_id(trimmed), trimmed.to_string()))
}

fn normalize_subgraph_id(raw: &str) -> String {
    let mut id = raw.trim().to_string();
    if id.is_empty() {
        id = "subgraph".to_string();
    }
    id.chars()
        .map(|ch| if ch.is_whitespace() { '_' } else { ch })
        .collect()
}

fn record_node_membership(
    node_id: &str,
    subgraph_stack: &mut Vec<SubgraphBuilder>,
    node_membership: &mut HashMap<String, Vec<String>>,
) {
    let node_id_string = node_id.to_string();

    // Record membership path using current stack ordering.
    let path: Vec<String> = subgraph_stack.iter().map(|sg| sg.id.clone()).collect();
    node_membership.insert(node_id_string.clone(), path);

    if let Some(current) = subgraph_stack.last_mut() {
        if !current.nodes.contains(&node_id_string) {
            current.nodes.push(node_id_string);
        }
    }
}

fn prune_node_from_subgraphs(subgraphs: &mut Vec<Subgraph>, node_id: &str) -> bool {
    subgraphs.retain_mut(|subgraph| {
        subgraph.nodes.retain(|id| id != node_id);
        prune_node_from_subgraphs(&mut subgraph.children, node_id);
        !subgraph.nodes.is_empty() || !subgraph.children.is_empty()
    });
    !subgraphs.is_empty()
}

fn parse_node_line(
    line: &str,
    nodes: &mut HashMap<String, Node>,
    order: &mut Vec<String>,
    node_membership: &mut HashMap<String, Vec<String>>,
    subgraph_stack: &mut Vec<SubgraphBuilder>,
) -> Result<bool> {
    if line.contains("-->") || line.contains("-.->") {
        return Ok(false);
    }

    let spec = match NodeSpec::parse(line) {
        Ok(spec) => spec,
        Err(_) => return Ok(false),
    };

    let (id, inserted) = insert_node_spec(spec, nodes, order);
    if inserted {
        record_node_membership(&id, subgraph_stack, node_membership);
    } else if !node_membership.contains_key(&id) {
        if subgraph_stack.is_empty() {
            node_membership.insert(id.clone(), Vec::new());
        } else {
            // Preserve membership for nodes first declared outside any subgraph when later wrapped.
            record_node_membership(&id, subgraph_stack, node_membership);
        }
    }

    Ok(true)
}

fn parse_edge_line(
    line: &str,
    nodes: &mut HashMap<String, Node>,
    order: &mut Vec<String>,
    node_membership: &mut HashMap<String, Vec<String>>,
    subgraph_stack: &mut Vec<SubgraphBuilder>,
) -> Result<Option<Edge>> {
    const EDGE_PATTERNS: [(&str, EdgeKind); 2] =
        [("-.->", EdgeKind::Dashed), ("-->", EdgeKind::Solid)];

    let mut parts = None;
    for (pattern, kind) in EDGE_PATTERNS {
        if let Some((lhs, rhs)) = line.split_once(pattern) {
            parts = Some((lhs.trim(), rhs.trim(), kind));
            break;
        }
    }

    let Some((lhs, rhs, kind)) = parts else {
        return Ok(None);
    };

    let (label, rhs_clean) = if let Some(rest) = rhs.strip_prefix('|') {
        let Some(end_idx) = rest.find('|') else {
            bail!("edge label missing closing '|' in line: '{line}'");
        };
        let label = rest[..end_idx].trim();
        let target = rest[end_idx + 1..].trim();
        (Some(label.to_string()), target)
    } else {
        (None, rhs)
    };

    let (from_id, from_new) = intern_node(lhs, nodes, order)?;
    if from_new {
        record_node_membership(&from_id, subgraph_stack, node_membership);
    } else if !node_membership.contains_key(&from_id) && subgraph_stack.is_empty() {
        node_membership.insert(from_id.clone(), Vec::new());
    }

    let (to_id, to_new) = intern_node(rhs_clean, nodes, order)?;
    if to_new {
        record_node_membership(&to_id, subgraph_stack, node_membership);
    } else if !node_membership.contains_key(&to_id) && subgraph_stack.is_empty() {
        node_membership.insert(to_id.clone(), Vec::new());
    }

    Ok(Some(Edge {
        from: from_id,
        to: to_id,
        label,
        kind,
    }))
}

fn intern_node(
    raw: &str,
    nodes: &mut HashMap<String, Node>,
    order: &mut Vec<String>,
) -> Result<(String, bool)> {
    let spec = NodeSpec::parse(raw)?;
    Ok(insert_node_spec(spec, nodes, order))
}

fn insert_node_spec(
    spec: NodeSpec,
    nodes: &mut HashMap<String, Node>,
    order: &mut Vec<String>,
) -> (String, bool) {
    let NodeSpec { id, label, shape } = spec;
    let mut inserted = false;
    match nodes.entry(id.clone()) {
        Entry::Vacant(entry) => {
            order.push(id.clone());
            entry.insert(Node { label, shape });
            inserted = true;
        }
        Entry::Occupied(_) => {}
    }
    (id, inserted)
}

struct NodeSpec {
    id: String,
    label: String,
    shape: NodeShape,
}

impl NodeSpec {
    fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("encountered empty node reference");
        }

        let mut id_end = trimmed.len();
        for (idx, ch) in trimmed.char_indices() {
            if matches!(ch, '[' | '(' | '{') {
                id_end = idx;
                break;
            }
        }

        let id = trimmed[..id_end].trim();
        if id.is_empty() {
            bail!("node identifier missing in segment '{trimmed}'");
        }

        let remainder = trimmed[id_end..].trim();
        let (label, shape) = if remainder.is_empty() {
            (id.to_string(), NodeShape::Rectangle)
        } else if remainder.starts_with("((") && remainder.ends_with("))") && remainder.len() >= 4 {
            (
                remainder[2..remainder.len() - 2].trim().to_string(),
                NodeShape::Circle,
            )
        } else if remainder.starts_with('(') && remainder.ends_with(')') && remainder.len() >= 2 {
            (
                remainder[1..remainder.len() - 1].trim().to_string(),
                NodeShape::Stadium,
            )
        } else if remainder.starts_with('[') && remainder.ends_with(']') && remainder.len() >= 2 {
            (
                remainder[1..remainder.len() - 1].trim().to_string(),
                NodeShape::Rectangle,
            )
        } else if remainder.starts_with('{') && remainder.ends_with('}') && remainder.len() >= 2 {
            (
                remainder[1..remainder.len() - 1].trim().to_string(),
                NodeShape::Diamond,
            )
        } else {
            (trimmed.to_string(), NodeShape::Rectangle)
        };

        Ok(NodeSpec {
            id: id.to_string(),
            label: if label.is_empty() {
                id.to_string()
            } else {
                label
            },
            shape,
        })
    }
}
