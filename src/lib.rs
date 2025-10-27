use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

pub mod diagram;
pub mod serve;
pub mod utils;

pub use diagram::*;
pub use utils::*;

pub const NODE_WIDTH: f32 = 140.0;
pub const NODE_HEIGHT: f32 = 60.0;
pub const NODE_SPACING: f32 = 160.0;
pub const START_OFFSET: f32 = 120.0;
pub const LAYOUT_MARGIN: f32 = 80.0;
pub const EDGE_LABEL_MIN_WIDTH: f32 = 36.0;
pub const EDGE_LABEL_MIN_HEIGHT: f32 = 28.0;
pub const EDGE_LABEL_LINE_HEIGHT: f32 = 16.0;
pub const EDGE_LABEL_HORIZONTAL_PADDING: f32 = 16.0;
pub const EDGE_LABEL_VERTICAL_PADDING: f32 = 12.0;
pub const EDGE_LABEL_CHAR_WIDTH: f32 = 7.4;
pub const EDGE_LABEL_VERTICAL_OFFSET: f32 = 10.0;
pub const EDGE_BIDIRECTIONAL_OFFSET: f32 = 28.0;
pub const EDGE_BIDIRECTIONAL_STUB: f32 = 48.0;
pub const EDGE_BIDIRECTIONAL_OFFSET_STEP: f32 = 12.0;
pub const EDGE_BIDIRECTIONAL_STUB_STEP: f32 = 18.0;
pub const EDGE_COLLISION_MARGIN: f32 = 6.0;
pub const EDGE_COLLISION_MAX_ITER: usize = 6;
pub const EDGE_SINGLE_OFFSET: f32 = 32.0;
pub const EDGE_SINGLE_STUB: f32 = 56.0;
pub const EDGE_SINGLE_OFFSET_STEP: f32 = 14.0;
pub const EDGE_SINGLE_STUB_STEP: f32 = 20.0;
pub const EDGE_ARROW_EXTENSION: f32 = 1.0;
pub const LAYOUT_BLOCK_START: &str = "%% OXDRAW LAYOUT START";
pub const LAYOUT_BLOCK_END: &str = "%% OXDRAW LAYOUT END";
pub const SUBGRAPH_PADDING: f32 = 48.0;
pub const SUBGRAPH_LABEL_AREA: f32 = 36.0;
pub const SUBGRAPH_LABEL_TEXT_BASELINE: f32 = 20.0;
pub const SUBGRAPH_LABEL_INSET_X: f32 = 20.0;
pub const SUBGRAPH_SEPARATION: f32 = 140.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeOverride {
    #[serde(default)]
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeStyleOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeStyleOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<EdgeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrow: Option<EdgeArrowDirection>,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    TopDown,
    LeftRight,
    BottomTop,
    RightLeft,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub label: String,
    pub shape: NodeShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,
    Stadium,
    Circle,
    DoubleCircle,
    Diamond,
    Subroutine,
    Cylinder,
    Hexagon,
    Parallelogram,
    ParallelogramAlt,
    Trapezoid,
    TrapezoidAlt,
    Asymmetric,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    Solid,
    Dashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeArrowDirection {
    Forward,
    Backward,
    Both,
    None,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CanvasSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct AutoLayout {
    pub positions: HashMap<String, Point>,
    pub size: CanvasSize,
}

#[derive(Debug, Clone)]
pub struct LayoutComputation {
    pub auto_positions: HashMap<String, Point>,
    pub auto_routes: HashMap<String, Vec<Point>>,
    pub auto_size: CanvasSize,
    pub final_positions: HashMap<String, Point>,
    pub final_routes: HashMap<String, Vec<Point>>,
}

#[derive(Debug, Clone)]
pub struct Geometry {
    pub positions: HashMap<String, Point>,
    pub edges: HashMap<String, Vec<Point>>,
    pub subgraphs: Vec<SubgraphVisual>,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct SubgraphVisual {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label_x: f32,
    pub label_y: f32,
    pub depth: usize,
    pub order: usize,
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct NodeStylePatch {
    #[serde(default)]
    pub fill: Option<Option<String>>,
    #[serde(default)]
    pub stroke: Option<Option<String>>,
    #[serde(default)]
    pub text: Option<Option<String>>,
}

impl NodeStyleOverride {
    pub fn is_empty(&self) -> bool {
        self.fill.is_none() && self.stroke.is_none() && self.text.is_none()
    }
}

impl EdgeStyleOverride {
    pub fn is_empty(&self) -> bool {
        self.line.is_none() && self.color.is_none() && self.arrow.is_none()
    }
}

impl Default for EdgeArrowDirection {
    fn default() -> Self {
        EdgeArrowDirection::Forward
    }
}

impl EdgeArrowDirection {
    pub fn marker_start(self) -> bool {
        matches!(
            self,
            EdgeArrowDirection::Backward | EdgeArrowDirection::Both
        )
    }

    pub fn marker_end(self) -> bool {
        matches!(self, EdgeArrowDirection::Forward | EdgeArrowDirection::Both)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeArrowDirection::Forward => "forward",
            EdgeArrowDirection::Backward => "backward",
            EdgeArrowDirection::Both => "both",
            EdgeArrowDirection::None => "none",
        }
    }
}
