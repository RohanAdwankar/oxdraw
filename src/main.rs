use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as FmtWrite;
use std::io::{Write};
use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderValue,header};
use axum::response::{Response};
use serde::{Deserialize, Serialize};

mod cli;
use crate::cli::*;
mod utils;
use crate::utils::escape_xml;
mod serve;
mod diagram;
use crate::diagram::*;

const NODE_WIDTH: f32 = 140.0;
const NODE_HEIGHT: f32 = 60.0;
const NODE_SPACING: f32 = 160.0;
const START_OFFSET: f32 = 120.0;
const LAYOUT_MARGIN: f32 = 80.0;
const LAYOUT_BLOCK_START: &str = "%% OXDRAW LAYOUT START";
const LAYOUT_BLOCK_END: &str = "%% OXDRAW LAYOUT END";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EdgeOverride {
    #[serde(default)]
    points: Vec<Point>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NodeStyleOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EdgeStyleOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<EdgeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrow: Option<EdgeArrowDirection>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = dispatch().await {
        eprintln!("\u{001b}[31merror:\u{001b}[0m {err:?}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    TopDown,
    LeftRight,
    BottomTop,
    RightLeft,
}

#[derive(Debug, Clone)]
struct Node {
    label: String,
    shape: NodeShape,
}

#[derive(Debug, Clone, Copy)]
enum NodeShape {
    Rectangle,
    Stadium,
    Circle,
    Diamond,
}

#[derive(Debug, Clone)]
struct Edge {
    from: String,
    to: String,
    label: Option<String>,
    kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EdgeKind {
    Solid,
    Dashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EdgeArrowDirection {
    Forward,
    Backward,
    Both,
    None,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct CanvasSize {
    width: f32,
    height: f32,
}

#[derive(Debug, Clone)]
struct AutoLayout {
    positions: HashMap<String, Point>,
    size: CanvasSize,
}

#[derive(Debug, Clone)]
struct LayoutComputation {
    auto_positions: HashMap<String, Point>,
    auto_routes: HashMap<String, Vec<Point>>,
    auto_size: CanvasSize,
    final_positions: HashMap<String, Point>,
    final_routes: HashMap<String, Vec<Point>>,
}

#[derive(Debug, Clone)]
struct Geometry {
    positions: HashMap<String, Point>,
    edges: HashMap<String, Vec<Point>>,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize, Default)]
struct NodeStylePatch {
    #[serde(default)]
    fill: Option<Option<String>>,
    #[serde(default)]
    stroke: Option<Option<String>>,
    #[serde(default)]
    text: Option<Option<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct EdgeStylePatch {
    #[serde(default)]
    line: Option<Option<EdgeKind>>,
    #[serde(default)]
    color: Option<Option<String>>,
    #[serde(default)]
    arrow: Option<Option<EdgeArrowDirection>>,
}

#[derive(Debug, Serialize)]
struct SourcePayload {
    source: String,
}

impl NodeStyleOverride {
    fn is_empty(&self) -> bool {
        self.fill.is_none() && self.stroke.is_none() && self.text.is_none()
    }
}

impl EdgeStyleOverride {
    fn is_empty(&self) -> bool {
        self.line.is_none() && self.color.is_none() && self.arrow.is_none()
    }
}

impl Default for EdgeArrowDirection {
    fn default() -> Self {
        EdgeArrowDirection::Forward
    }
}

impl EdgeArrowDirection {
    fn marker_start(self) -> bool {
        matches!(
            self,
            EdgeArrowDirection::Backward | EdgeArrowDirection::Both
        )
    }

    fn marker_end(self) -> bool {
        matches!(self, EdgeArrowDirection::Forward | EdgeArrowDirection::Both)
    }

    fn as_str(self) -> &'static str {
        match self {
            EdgeArrowDirection::Forward => "forward",
            EdgeArrowDirection::Backward => "backward",
            EdgeArrowDirection::Both => "both",
            EdgeArrowDirection::None => "none",
        }
    }
}

fn split_source_and_overrides(source: &str) -> Result<(String, LayoutOverrides)> {
    let mut definition_lines = Vec::new();
    let mut layout_lines = Vec::new();
    let mut in_block = false;
    let mut found_block = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(LAYOUT_BLOCK_START) {
            if in_block {
                bail!("nested '{}' sections are not supported", LAYOUT_BLOCK_START);
            }
            in_block = true;
            found_block = true;
            continue;
        }
        if trimmed.eq_ignore_ascii_case(LAYOUT_BLOCK_END) {
            if !in_block {
                bail!(
                    "encountered '{}' without a matching start",
                    LAYOUT_BLOCK_END
                );
            }
            in_block = false;
            continue;
        }

        if in_block {
            if trimmed.is_empty() {
                continue;
            }
            let mut segment = line.trim_start();
            if let Some(rest) = segment.strip_prefix("%%") {
                segment = rest.trim_start();
            }
            layout_lines.push(segment.to_string());
        } else {
            definition_lines.push(line);
        }
    }

    if in_block {
        bail!(
            "layout metadata block was not terminated with '{}'",
            LAYOUT_BLOCK_END
        );
    }

    let mut definition = definition_lines.join("\n");
    if source.ends_with('\n') {
        definition.push('\n');
    }

    let overrides = if found_block {
        let json = layout_lines.join("\n");
        if json.trim().is_empty() {
            LayoutOverrides::default()
        } else {
            serde_json::from_str(&json)
                .with_context(|| "failed to parse embedded oxdraw layout block")?
        }
    } else {
        LayoutOverrides::default()
    };

    Ok((definition, overrides))
}

fn merge_source_and_overrides(definition: &str, overrides: &LayoutOverrides) -> Result<String> {
    let trimmed = definition.trim_end_matches('\n');
    let mut output = trimmed.to_string();
    output.push('\n');

    if overrides.is_empty() {
        return Ok(output);
    }

    output.push('\n');
    output.push_str(LAYOUT_BLOCK_START);
    output.push('\n');

    let json = serde_json::to_string_pretty(overrides)?;
    for line in json.lines() {
        output.push_str("%% ");
        output.push_str(line);
        output.push('\n');
    }

    output.push_str(LAYOUT_BLOCK_END);
    output.push('\n');

    Ok(output)
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

fn edge_identifier(edge: &Edge) -> String {
    format!("{} {} {}", edge.from, edge.kind.arrow_token(), edge.to)
}

fn centroid(points: &[Point]) -> Point {
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

fn align_geometry(
    positions: &HashMap<String, Point>,
    routes: &HashMap<String, Vec<Point>>,
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
    fn arrow_token(&self) -> &'static str {
        match self {
            EdgeKind::Solid => "-->",
            EdgeKind::Dashed => "-.->",
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Solid => "solid",
            EdgeKind::Dashed => "dashed",
        }
    }
}

impl NodeShape {
    fn as_str(&self) -> &'static str {
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

