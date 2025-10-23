use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
pub struct Diagram {
    pub direction: Direction,
    pub nodes: HashMap<String, Node>,
    pub order: Vec<String>,
    pub edges: Vec<Edge>,
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

        for line in lines {
            if let Some(edge) = parse_edge_line(line, &mut nodes, &mut order)? {
                edges.push(edge);
            }
        }

        if nodes.is_empty() {
            bail!("diagram does not declare any nodes");
        }

        Ok(Self {
            direction,
            nodes,
            order,
            edges,
        })
    }

    pub fn render_svg(
        &self,
        background: &str,
        overrides: Option<&LayoutOverrides>,
    ) -> Result<String> {
        let layout = self.layout(overrides)?;
        let geometry = align_geometry(&layout.final_positions, &layout.final_routes, &self.edges)?;

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
        let auto = self.compute_auto_layout();
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

    fn compute_routes(
        &self,
        positions: &HashMap<String, Point>,
        overrides: Option<&LayoutOverrides>,
    ) -> Result<HashMap<String, Vec<Point>>> {
        let mut routes = HashMap::new();
        let mut edge_ids = Vec::with_capacity(self.edges.len());
        let mut pairings: HashMap<(String, String), Vec<(usize, bool)>> = HashMap::new();

        let mut node_rects: HashMap<String, Rect> = HashMap::new();
        for (id, point) in positions {
            node_rects.insert(id.clone(), node_rect(*point));
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

            let base_label_collision = self.label_collides_with_nodes(edge, &path, &node_rects);
            let base_intersections = count_route_intersections(&path, &routes);

            if middle_points.is_empty() && !has_override(edge_idx) {
                if let Some(adjusted) = self.adjust_edge_for_conflicts(
                    from,
                    to,
                    edge,
                    &node_rects,
                    &routes,
                    base_label_collision,
                    base_intersections,
                ) {
                    path = build_route(from, &adjusted, to);
                }
            }

            if has_custom_override {
                if let Some(custom) = overrides.and_then(|ov| ov.edges.get(edge_id)) {
                    path = build_route(from, &custom.points, to);
                }
            }

            if let (Some(&from_rect), Some(&to_rect)) =
                (node_rects.get(&edge.from), node_rects.get(&edge.to))
            {
                trim_route_endpoints(&mut path, from_rect, to_rect);
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
        node_rects: &HashMap<String, Rect>,
        existing_routes: &HashMap<String, Vec<Point>>,
        base_label_collision: bool,
        base_intersections: usize,
    ) -> Option<Vec<Point>> {
        let base_metric = ((base_label_collision as u8), base_intersections);
        if base_metric == (0_u8, 0_usize) {
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

        let base_offset = (distance * 0.25).min(EDGE_SINGLE_OFFSET).min(max_offset);
        let base_stub = (distance * 0.25).min(EDGE_SINGLE_STUB).min(max_stub);

        if base_offset <= 0.0 || base_stub <= 0.0 {
            return None;
        }

        let mut best: Option<(Vec<Point>, (u8, usize))> = None;

        for &normal_sign in &[1.0, -1.0] {
            for attempt in 0..=EDGE_COLLISION_MAX_ITER {
                let offset = (base_offset + attempt as f32 * EDGE_SINGLE_OFFSET_STEP)
                    .min(max_offset)
                    .max(base_offset);
                let stub = (base_stub + attempt as f32 * EDGE_SINGLE_STUB_STEP)
                    .min(max_stub)
                    .max(base_stub);

                let points = Diagram::generate_bidir_points(from, to, offset, stub, normal_sign);
                let route = build_route(from, &points, to);

                if self.label_collides_with_nodes(edge, &route, node_rects) {
                    continue;
                }

                let intersection_count = count_route_intersections(&route, existing_routes);
                let candidate_metric = (0_u8, intersection_count);

                if candidate_metric == (0_u8, 0_usize) {
                    return Some(points);
                }

                if candidate_metric < base_metric {
                    match &mut best {
                        Some((existing_points, existing_metric)) => {
                            if candidate_metric < *existing_metric {
                                *existing_points = points;
                                *existing_metric = candidate_metric;
                            }
                        }
                        None => best = Some((points, candidate_metric)),
                    }
                }

                if (offset - max_offset).abs() < f32::EPSILON
                    && (stub - max_stub).abs() < f32::EPSILON
                {
                    break;
                }
            }
        }

        best.map(|(points, _)| points)
    }

    fn label_collides_with_nodes(
        &self,
        edge: &Edge,
        route: &[Point],
        node_rects: &HashMap<String, Rect>,
    ) -> bool {
        let rect = match label_rect_for_route(edge, route) {
            Some(rect) => rect.inflate(EDGE_COLLISION_MARGIN),
            None => return false,
        };

        node_rects
            .values()
            .any(|node_rect| rect.intersects(node_rect))
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

        for id in &self.order {
            if let Some(node) = self.nodes.get(id) {
                lines.push(Self::format_node_line(id, node));
            }
        }

        for edge in &self.edges {
            lines.push(Self::format_edge_line(edge));
        }

        let mut output = lines.join("\n");
        output.push('\n');
        output
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
}

fn trim_route_endpoints(path: &mut Vec<Point>, from_rect: Rect, to_rect: Rect) {
    if path.len() < 2 {
        return;
    }

    if from_rect.contains(path[0]) {
        if let Some(trimmed) = clip_segment_exit(path[0], path[1], from_rect, false) {
            path[0] = trimmed;
        }
    }

    if path.len() < 2 {
        return;
    }

    let last = path.len() - 1;
    if to_rect.contains(path[last]) {
        if let Some(trimmed) = clip_segment_exit(path[last], path[last - 1], to_rect, true) {
            path[last] = trimmed;
        }
    }
}

fn clip_segment_exit(start: Point, next: Point, rect: Rect, extend_outward: bool) -> Option<Point> {
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

    Ok(Geometry {
        positions: shifted_positions,
        edges: shifted_routes,
        width,
        height,
    })
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

fn parse_edge_line(
    line: &str,
    nodes: &mut HashMap<String, Node>,
    order: &mut Vec<String>,
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

    let from_id = intern_node(lhs, nodes, order)?;
    let to_id = intern_node(rhs_clean, nodes, order)?;

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
) -> Result<String> {
    let spec = NodeSpec::parse(raw)?;
    match nodes.entry(spec.id.clone()) {
        Entry::Vacant(entry) => {
            order.push(spec.id.clone());
            entry.insert(Node {
                label: spec.label,
                shape: spec.shape,
            });
        }
        Entry::Occupied(_) => {}
    }
    Ok(spec.id)
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
