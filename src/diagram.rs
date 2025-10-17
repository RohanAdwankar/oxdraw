use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

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

    pub fn render_svg(&self, background: &str, overrides: Option<&LayoutOverrides>) -> Result<String> {
        let layout = self.layout(overrides)?;
        let geometry = align_geometry(&layout.final_positions, &layout.final_routes)?;

        let mut svg = String::new();
        write!(
            svg,
            r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" viewBox="0 0 {:.0} {:.0}" font-family="Inter, system-ui, sans-serif">
  <defs>
        <marker id="arrow-end" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto" markerUnits="strokeWidth">
            <path d="M2,2 L10,6 L2,10 z" fill="context-stroke" />
        </marker>
        <marker id="arrow-start" markerWidth="12" markerHeight="12" refX="2" refY="6" orient="auto" markerUnits="strokeWidth">
            <path d="M10,2 L2,6 L10,10 z" fill="context-stroke" />
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
                let centroid = centroid(&route);
                write!(
                    svg,
                    "  <text x=\"{:.1}\" y=\"{:.1}\" fill=\"#2d3748\" font-size=\"13\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>\n",
                    centroid.x,
                    centroid.y - 10.0,
                    escape_xml(label)
                )?;
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

        for edge in &self.edges {
            let from = *positions
                .get(&edge.from)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", edge.from))?;
            let to = *positions
                .get(&edge.to)
                .ok_or_else(|| anyhow!("edge references unknown node '{}'", edge.to))?;

            let mut path = Vec::new();
            path.push(from);

            if let Some(overrides) = overrides {
                if let Some(custom) = overrides.edges.get(&edge_identifier(edge)) {
                    for point in &custom.points {
                        path.push(*point);
                    }
                }
            }

            path.push(to);
            routes.insert(edge_identifier(edge), path);
        }

        Ok(routes)
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
