use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{Write};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

mod cli;
use crate::cli::*;
mod utils;
use crate::utils::{escape_xml};
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

