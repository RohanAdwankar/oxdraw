use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::diagram::{LayoutOverrides, Point, align_geometry, edge_identifier};
use crate::utils::split_source_and_overrides;
use crate::{CanvasSize, Diagram, DiagramKind, EdgeArrowDirection, EdgeKind, EdgeOverride};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramViewModel {
    pub kind: String,
    pub background: String,
    pub auto_size: CanvasSize,
    pub render_size: CanvasSize,
    pub nodes: Vec<NodeViewModel>,
    pub edges: Vec<EdgeViewModel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subgraphs: Vec<SubgraphViewModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gantt: Option<GanttViewModel>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeViewModel {
    pub id: String,
    pub label: String,
    pub shape: String,
    pub auto_position: Point,
    pub rendered_position: Point,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_fill_color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub membership: Vec<String>,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphViewModel {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeViewModel {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub kind: String,
    pub auto_points: Vec<Point>,
    pub rendered_points: Vec<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_points: Option<Vec<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrow_direction: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GanttViewModel {
    pub date_format: String,
    pub title: Option<String>,
    pub min_day: f64,
    pub max_day: f64,
    pub section_label_width: f32,
    pub timeline_width: f32,
    pub top_margin: f32,
    pub row_height: f32,
    pub bar_height: f32,
    pub right_padding: f32,
    pub bottom_margin: f32,
    pub sections: Vec<String>,
    pub tasks: Vec<GanttTaskViewModel>,
    pub style: GanttStyleViewModel,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GanttTaskViewModel {
    pub id: String,
    pub label: String,
    pub section_index: usize,
    pub row_index: usize,
    pub start_day: f64,
    pub end_day: f64,
    pub milestone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GanttStyleViewModel {
    pub row_fill_even: String,
    pub row_fill_odd: String,
    pub task_fill: String,
    pub milestone_fill: String,
    pub task_text: String,
    pub milestone_text: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LayoutUpdateInput {
    #[serde(default)]
    pub nodes: HashMap<String, Option<Point>>,
    #[serde(default)]
    pub edges: HashMap<String, Option<EdgeOverride>>,
    #[serde(default)]
    pub gantt_tasks: HashMap<String, Option<GanttTaskUpdateInput>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GanttTaskUpdateInput {
    #[serde(default)]
    pub start_day: Option<f64>,
    #[serde(default)]
    pub end_day: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StyleUpdateInput {
    #[serde(default)]
    pub node_styles: HashMap<String, Option<NodeStylePatchInput>>,
    #[serde(default)]
    pub edge_styles: HashMap<String, Option<EdgeStylePatchInput>>,
    #[serde(default)]
    pub gantt_style: Option<GanttStylePatchInput>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeStylePatchInput {
    #[serde(default)]
    pub fill: Option<Option<String>>,
    #[serde(default)]
    pub stroke: Option<Option<String>>,
    #[serde(default)]
    pub text: Option<Option<String>>,
    #[serde(default)]
    pub label_fill: Option<Option<String>>,
    #[serde(default)]
    pub image_fill: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EdgeStylePatchInput {
    #[serde(default)]
    pub line: Option<Option<EdgeKind>>,
    #[serde(default)]
    pub color: Option<Option<String>>,
    #[serde(default)]
    pub arrow: Option<Option<EdgeArrowDirection>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GanttStylePatchInput {
    #[serde(default)]
    pub row_fill_even: Option<Option<String>>,
    #[serde(default)]
    pub row_fill_odd: Option<Option<String>>,
    #[serde(default)]
    pub task_fill: Option<Option<String>>,
    #[serde(default)]
    pub milestone_fill: Option<Option<String>>,
    #[serde(default)]
    pub milestone_text: Option<Option<String>>,
    #[serde(default)]
    pub task_text: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct EditorCore {
    definition: String,
    overrides: LayoutOverrides,
    background: String,
    drag_state: Option<DragState>,
}

#[derive(Debug, Clone)]
enum DragState {
    Node(NodeDragState),
    Edge(EdgeDragState),
    Subgraph(SubgraphDragState),
    GanttTask(GanttTaskDragState),
}

#[derive(Debug, Clone)]
struct NodeDragState {
    id: String,
    offset: Point,
    current: Point,
    moved: bool,
}

#[derive(Debug, Clone)]
struct EdgeDragState {
    id: String,
    index: usize,
    points: Vec<Point>,
    moved: bool,
}

#[derive(Debug, Clone)]
struct SubgraphDragState {
    offset: Point,
    origin: Point,
    delta: Point,
    members: Vec<String>,
    base_positions: HashMap<String, Point>,
    auto_positions: HashMap<String, Point>,
    moved: bool,
}

#[derive(Debug, Clone, Copy)]
enum GanttDragMode {
    Move,
    ResizeStart,
    ResizeEnd,
    Milestone,
}

#[derive(Debug, Clone)]
struct GanttTaskDragState {
    id: String,
    mode: GanttDragMode,
    min_day: f64,
    max_day: f64,
    section_label_width: f32,
    timeline_width: f32,
    grab_offset_day: f64,
    start_day: f64,
    end_day: f64,
    moved: bool,
}

impl EditorCore {
    pub fn from_source(source: &str, background: impl Into<String>) -> Result<Self> {
        let (definition, overrides) = split_source_and_overrides(source)?;
        Ok(Self {
            definition,
            overrides,
            background: background.into(),
            drag_state: None,
        })
    }

    pub fn from_parts(
        definition: impl Into<String>,
        overrides: LayoutOverrides,
        background: impl Into<String>,
    ) -> Self {
        Self {
            definition: definition.into(),
            overrides,
            background: background.into(),
            drag_state: None,
        }
    }

    pub fn source(&self) -> Result<String> {
        merge_source_and_overrides(&self.definition, &self.overrides)
    }

    pub fn render_svg(&self) -> Result<String> {
        let diagram = Diagram::parse(&self.definition)?;
        let effective = self.effective_overrides();
        let override_ref = if effective.is_empty() {
            None
        } else {
            Some(&effective)
        };
        diagram.render_svg(&self.background, override_ref)
    }

    pub fn view_model(&self) -> Result<DiagramViewModel> {
        let diagram = Diagram::parse(&self.definition)?;
        let effective_overrides = self.effective_overrides();
        let layout = diagram.layout(Some(&effective_overrides))?;
        let geometry = align_geometry(
            &layout.final_positions,
            &layout.final_routes,
            &diagram.edges,
            &diagram.subgraphs,
            &diagram.nodes,
        )?;

        let mut nodes = Vec::new();
        for id in &diagram.order {
            let node = diagram
                .nodes
                .get(id)
                .ok_or_else(|| anyhow!("node '{id}' missing from diagram"))?;
            let auto_position = layout
                .auto_positions
                .get(id)
                .copied()
                .ok_or_else(|| anyhow!("auto layout missing node '{id}'"))?;
            let final_position = layout
                .final_positions
                .get(id)
                .copied()
                .ok_or_else(|| anyhow!("final layout missing node '{id}'"))?;
            let override_position = effective_overrides.nodes.get(id).copied();
            let style = self.overrides.node_styles.get(id);

            nodes.push(NodeViewModel {
                id: id.clone(),
                label: node.label.clone(),
                shape: node.shape.as_str().to_string(),
                auto_position,
                rendered_position: final_position,
                override_position,
                fill_color: style.and_then(|s| s.fill.clone()),
                stroke_color: style.and_then(|s| s.stroke.clone()),
                text_color: style.and_then(|s| s.text.clone()),
                label_fill_color: style.and_then(|s| s.label_fill.clone()),
                image_fill_color: style.and_then(|s| s.image_fill.clone()),
                membership: diagram.node_membership.get(id).cloned().unwrap_or_default(),
                width: node.width,
                height: node.height,
            });
        }

        let mut edges = Vec::new();
        for edge in &diagram.edges {
            let identifier = edge_identifier(edge);
            let auto_points = layout
                .auto_routes
                .get(&identifier)
                .cloned()
                .ok_or_else(|| anyhow!("auto route missing edge '{identifier}'"))?;
            let final_points = layout
                .final_routes
                .get(&identifier)
                .cloned()
                .ok_or_else(|| anyhow!("final route missing edge '{identifier}'"))?;
            let manual_points = effective_overrides
                .edges
                .get(&identifier)
                .map(|custom| custom.points.clone())
                .filter(|points| !points.is_empty());
            let style = self.overrides.edge_styles.get(&identifier);

            edges.push(EdgeViewModel {
                id: identifier,
                from: edge.from.clone(),
                to: edge.to.clone(),
                label: edge.label.clone(),
                kind: edge.kind.as_str().to_string(),
                auto_points,
                rendered_points: final_points,
                override_points: manual_points,
                color: style.and_then(|s| s.color.clone()),
                arrow_direction: style
                    .and_then(|s| s.arrow)
                    .map(|direction| direction.as_str().to_string()),
            });
        }

        let subgraphs = geometry
            .subgraphs
            .iter()
            .map(|subgraph| SubgraphViewModel {
                id: subgraph.id.clone(),
                label: subgraph.label.clone(),
                x: subgraph.x,
                y: subgraph.y,
                width: subgraph.width,
                height: subgraph.height,
                label_x: subgraph.label_x,
                label_y: subgraph.label_y,
                depth: subgraph.depth,
                order: subgraph.order,
                parent_id: subgraph.parent_id.clone(),
            })
            .collect();

        let (kind, gantt) = match &diagram.kind {
            DiagramKind::Flowchart => ("flowchart".to_string(), None),
            DiagramKind::Gantt(gantt) => {
                let gantt_overrides = &self.overrides.gantt;
                let row_fill_even = gantt_overrides
                    .style
                    .row_fill_even
                    .clone()
                    .unwrap_or_else(|| "#f8fafc".to_string());
                let row_fill_odd = gantt_overrides
                    .style
                    .row_fill_odd
                    .clone()
                    .unwrap_or_else(|| "#eef2ff".to_string());
                let task_fill = gantt_overrides
                    .style
                    .task_fill
                    .clone()
                    .unwrap_or_else(|| "#60a5fa".to_string());
                let milestone_fill = gantt_overrides
                    .style
                    .milestone_fill
                    .clone()
                    .unwrap_or_else(|| "#f97316".to_string());
                let task_text = gantt_overrides
                    .style
                    .task_text
                    .clone()
                    .unwrap_or_else(|| "#0f172a".to_string());
                let milestone_text = gantt_overrides
                    .style
                    .milestone_text
                    .clone()
                    .unwrap_or_else(|| "#7c2d12".to_string());

                let tasks: Vec<GanttTaskViewModel> = gantt
                    .tasks
                    .iter()
                    .enumerate()
                    .map(|(row_index, task)| {
                        let task_override = gantt_overrides.tasks.get(&task.id);
                        let start_day = task_override
                            .and_then(|patch| patch.start_day)
                            .unwrap_or(task.start_day);
                        let end_day = task_override
                            .and_then(|patch| patch.end_day)
                            .unwrap_or(task.end_day);

                        GanttTaskViewModel {
                            id: task.id.clone(),
                            label: task.label.clone(),
                            section_index: task.section_index,
                            row_index,
                            start_day,
                            end_day,
                            milestone: task.milestone,
                        }
                    })
                    .collect();

                let min_day = tasks
                    .iter()
                    .map(|task| task.start_day.min(task.end_day))
                    .fold(f64::INFINITY, f64::min);
                let max_day = tasks
                    .iter()
                    .map(|task| task.start_day.max(task.end_day))
                    .fold(f64::NEG_INFINITY, f64::max);

                (
                    "gantt".to_string(),
                    Some(GanttViewModel {
                        date_format: gantt.date_format.clone(),
                        title: gantt.title.clone(),
                        min_day,
                        max_day,
                        section_label_width: 160.0,
                        timeline_width: 1200.0,
                        top_margin: 68.0,
                        row_height: 40.0,
                        bar_height: 20.0,
                        right_padding: 40.0,
                        bottom_margin: 80.0,
                        sections: gantt.sections.clone(),
                        tasks,
                        style: GanttStyleViewModel {
                            row_fill_even,
                            row_fill_odd,
                            task_fill,
                            milestone_fill,
                            task_text,
                            milestone_text,
                        },
                    }),
                )
            }
        };

        Ok(DiagramViewModel {
            kind,
            background: self.background.clone(),
            auto_size: layout.auto_size,
            render_size: CanvasSize {
                width: geometry.width,
                height: geometry.height,
            },
            nodes,
            edges,
            subgraphs,
            gantt,
            source: self.source()?,
        })
    }

    pub fn apply_layout_update(&mut self, update: LayoutUpdateInput) {
        for (id, value) in update.nodes {
            match value {
                Some(point) => {
                    self.overrides.nodes.insert(id, point);
                }
                None => {
                    self.overrides.nodes.remove(&id);
                }
            }
        }

        for (id, value) in update.edges {
            match value {
                Some(edge_override) if !edge_override.points.is_empty() => {
                    self.overrides.edges.insert(id, edge_override);
                }
                _ => {
                    self.overrides.edges.remove(&id);
                }
            }
        }

        for (id, value) in update.gantt_tasks {
            match value {
                Some(task_update) => {
                    let mut current = self.overrides.gantt.tasks.remove(&id).unwrap_or_default();
                    if let Some(start) = task_update.start_day {
                        current.start_day = Some(start);
                    }
                    if let Some(end) = task_update.end_day {
                        current.end_day = Some(end);
                    }
                    if current.is_empty() {
                        self.overrides.gantt.tasks.remove(&id);
                    } else {
                        self.overrides.gantt.tasks.insert(id, current);
                    }
                }
                None => {
                    self.overrides.gantt.tasks.remove(&id);
                }
            }
        }
    }

    pub fn apply_style_update(&mut self, update: StyleUpdateInput) {
        for (id, value) in update.node_styles {
            match value {
                Some(patch) => {
                    let mut current = self.overrides.node_styles.remove(&id).unwrap_or_default();
                    if let Some(value) = patch.fill {
                        current.fill = value;
                    }
                    if let Some(value) = patch.stroke {
                        current.stroke = value;
                    }
                    if let Some(value) = patch.text {
                        current.text = value;
                    }
                    if let Some(value) = patch.label_fill {
                        current.label_fill = value;
                    }
                    if let Some(value) = patch.image_fill {
                        current.image_fill = value;
                    }
                    if current.is_empty() {
                        self.overrides.node_styles.remove(&id);
                    } else {
                        self.overrides.node_styles.insert(id, current);
                    }
                }
                None => {
                    self.overrides.node_styles.remove(&id);
                }
            }
        }

        for (id, value) in update.edge_styles {
            match value {
                Some(patch) => {
                    let mut current = self.overrides.edge_styles.remove(&id).unwrap_or_default();
                    if let Some(value) = patch.line {
                        current.line = value;
                    }
                    if let Some(value) = patch.color {
                        current.color = value;
                    }
                    if let Some(value) = patch.arrow {
                        current.arrow = value;
                    }
                    if current.is_empty() {
                        self.overrides.edge_styles.remove(&id);
                    } else {
                        self.overrides.edge_styles.insert(id, current);
                    }
                }
                None => {
                    self.overrides.edge_styles.remove(&id);
                }
            }
        }

        if let Some(patch) = update.gantt_style {
            if let Some(value) = patch.row_fill_even {
                self.overrides.gantt.style.row_fill_even = value;
            }
            if let Some(value) = patch.row_fill_odd {
                self.overrides.gantt.style.row_fill_odd = value;
            }
            if let Some(value) = patch.task_fill {
                self.overrides.gantt.style.task_fill = value;
            }
            if let Some(value) = patch.milestone_fill {
                self.overrides.gantt.style.milestone_fill = value;
            }
            if let Some(value) = patch.milestone_text {
                self.overrides.gantt.style.milestone_text = value;
            }
            if let Some(value) = patch.task_text {
                self.overrides.gantt.style.task_text = value;
            }
        }
    }

    pub fn set_source(&mut self, source: &str) -> Result<()> {
        let (definition, parsed_overrides) = split_source_and_overrides(source)?;
        let diagram = Diagram::parse(&definition)?;
        let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
        let edge_ids: HashSet<String> = diagram.edges.iter().map(edge_identifier).collect();

        self.definition = definition;
        self.overrides = parsed_overrides;
        self.overrides.prune(&node_ids, &edge_ids);
        self.drag_state = None;

        Ok(())
    }

    pub fn set_background(&mut self, background: impl Into<String>) {
        self.background = background.into();
    }

    pub fn background(&self) -> &str {
        &self.background
    }

    pub fn begin_node_drag(&mut self, id: &str, pointer_x: f32, pointer_y: f32) -> Result<()> {
        let diagram = Diagram::parse(&self.definition)?;
        let layout = diagram.layout(Some(&self.overrides))?;
        let current = layout
            .final_positions
            .get(id)
            .copied()
            .ok_or_else(|| anyhow!("node '{id}' not found"))?;

        self.drag_state = Some(DragState::Node(NodeDragState {
            id: id.to_string(),
            offset: Point {
                x: pointer_x - current.x,
                y: pointer_y - current.y,
            },
            current,
            moved: false,
        }));
        Ok(())
    }

    pub fn update_node_drag(&mut self, pointer_x: f32, pointer_y: f32) -> Result<()> {
        let Some(DragState::Node(mut drag)) = self.drag_state.clone() else {
            return Ok(());
        };

        let proposed = Point {
            x: pointer_x - drag.offset.x,
            y: pointer_y - drag.offset.y,
        };
        let snapped = Point {
            x: snap_to_grid(proposed.x),
            y: snap_to_grid(proposed.y),
        };
        drag.moved = drag.moved
            || (snapped.x - drag.current.x).abs() > f32::EPSILON
            || (snapped.y - drag.current.y).abs() > f32::EPSILON;
        drag.current = snapped;
        self.drag_state = Some(DragState::Node(drag));
        Ok(())
    }

    pub fn end_node_drag(&mut self) -> Result<Option<LayoutUpdateInput>> {
        let Some(DragState::Node(drag)) = self.drag_state.take() else {
            return Ok(None);
        };
        if !drag.moved {
            return Ok(None);
        }

        let diagram = Diagram::parse(&self.definition)?;
        let layout = diagram.layout(Some(&self.overrides))?;
        let auto = layout
            .auto_positions
            .get(&drag.id)
            .copied()
            .ok_or_else(|| anyhow!("auto position missing node '{}'", drag.id))?;

        let result = if points_close(drag.current, auto) {
            None
        } else {
            Some(drag.current)
        };

        self.overrides.nodes.remove(&drag.id);
        if let Some(point) = result {
            self.overrides.nodes.insert(drag.id.clone(), point);
        }

        let mut update = LayoutUpdateInput::default();
        update.nodes.insert(drag.id, result);
        Ok(Some(update))
    }

    pub fn begin_edge_drag(&mut self, id: &str, index: usize) -> Result<()> {
        let diagram = Diagram::parse(&self.definition)?;
        let layout = diagram.layout(Some(&self.overrides))?;

        let mut points = self
            .overrides
            .edges
            .get(id)
            .map(|override_points| override_points.points.clone())
            .unwrap_or_default();

        if points.is_empty() {
            let route = layout
                .final_routes
                .get(id)
                .ok_or_else(|| anyhow!("edge '{id}' not found"))?;
            let default_point = if route.len() >= 2 {
                let start = route.first().copied().unwrap_or(Point { x: 0.0, y: 0.0 });
                let end = route.last().copied().unwrap_or(Point { x: 0.0, y: 0.0 });
                Point {
                    x: (start.x + end.x) / 2.0,
                    y: (start.y + end.y) / 2.0,
                }
            } else {
                Point { x: 0.0, y: 0.0 }
            };
            points.push(default_point);
        }

        let bounded_index = index.min(points.len().saturating_sub(1));
        self.drag_state = Some(DragState::Edge(EdgeDragState {
            id: id.to_string(),
            index: bounded_index,
            points,
            moved: false,
        }));
        Ok(())
    }

    pub fn update_edge_drag(&mut self, pointer_x: f32, pointer_y: f32) -> Result<()> {
        let Some(DragState::Edge(mut drag)) = self.drag_state.clone() else {
            return Ok(());
        };
        let snapped = Point {
            x: snap_to_grid(pointer_x),
            y: snap_to_grid(pointer_y),
        };
        if let Some(current) = drag.points.get(drag.index).copied() {
            drag.moved = drag.moved
                || (snapped.x - current.x).abs() > f32::EPSILON
                || (snapped.y - current.y).abs() > f32::EPSILON;
        }
        if drag.index < drag.points.len() {
            drag.points[drag.index] = snapped;
        }
        self.drag_state = Some(DragState::Edge(drag));
        Ok(())
    }

    pub fn end_edge_drag(&mut self) -> Result<Option<LayoutUpdateInput>> {
        let Some(DragState::Edge(drag)) = self.drag_state.take() else {
            return Ok(None);
        };
        if !drag.moved {
            return Ok(None);
        }

        self.overrides.edges.insert(
            drag.id.clone(),
            EdgeOverride {
                points: drag.points.clone(),
            },
        );
        let mut update = LayoutUpdateInput::default();
        update.edges.insert(
            drag.id,
            Some(EdgeOverride {
                points: drag.points,
            }),
        );
        Ok(Some(update))
    }

    pub fn begin_subgraph_drag(&mut self, id: &str, pointer_x: f32, pointer_y: f32) -> Result<()> {
        let diagram = Diagram::parse(&self.definition)?;
        let effective = self.effective_overrides();
        let layout = diagram.layout(Some(&effective))?;
        let geometry = align_geometry(
            &layout.final_positions,
            &layout.final_routes,
            &diagram.edges,
            &diagram.subgraphs,
            &diagram.nodes,
        )?;

        let visual = geometry
            .subgraphs
            .iter()
            .find(|subgraph| subgraph.id == id)
            .ok_or_else(|| anyhow!("subgraph '{id}' not found"))?;

        let members: Vec<String> = diagram
            .nodes
            .keys()
            .filter(|node_id| {
                diagram
                    .node_membership
                    .get(*node_id)
                    .is_some_and(|path| path.iter().any(|entry| entry == id))
            })
            .cloned()
            .collect();

        if members.is_empty() {
            return Err(anyhow!("subgraph '{id}' has no draggable members"));
        }

        let mut base_positions = HashMap::new();
        let mut auto_positions = HashMap::new();
        for node_id in &members {
            let base = layout
                .final_positions
                .get(node_id)
                .copied()
                .ok_or_else(|| anyhow!("final position missing node '{node_id}'"))?;
            let auto = layout
                .auto_positions
                .get(node_id)
                .copied()
                .ok_or_else(|| anyhow!("auto position missing node '{node_id}'"))?;
            base_positions.insert(node_id.clone(), base);
            auto_positions.insert(node_id.clone(), auto);
        }

        self.drag_state = Some(DragState::Subgraph(SubgraphDragState {
            offset: Point {
                x: pointer_x - visual.x,
                y: pointer_y - visual.y,
            },
            origin: Point {
                x: visual.x,
                y: visual.y,
            },
            delta: Point { x: 0.0, y: 0.0 },
            members,
            base_positions,
            auto_positions,
            moved: false,
        }));
        Ok(())
    }

    pub fn update_subgraph_drag(&mut self, pointer_x: f32, pointer_y: f32) -> Result<()> {
        let Some(DragState::Subgraph(mut drag)) = self.drag_state.clone() else {
            return Ok(());
        };

        let next_origin = Point {
            x: snap_to_grid(pointer_x - drag.offset.x),
            y: snap_to_grid(pointer_y - drag.offset.y),
        };
        let next_delta = Point {
            x: next_origin.x - drag.origin.x,
            y: next_origin.y - drag.origin.y,
        };

        drag.moved = drag.moved
            || (next_delta.x - drag.delta.x).abs() > f32::EPSILON
            || (next_delta.y - drag.delta.y).abs() > f32::EPSILON;
        drag.delta = next_delta;
        self.drag_state = Some(DragState::Subgraph(drag));
        Ok(())
    }

    pub fn end_subgraph_drag(&mut self) -> Result<Option<LayoutUpdateInput>> {
        let Some(DragState::Subgraph(drag)) = self.drag_state.take() else {
            return Ok(None);
        };
        if !drag.moved {
            return Ok(None);
        }

        let mut update = LayoutUpdateInput::default();
        for node_id in &drag.members {
            let Some(base) = drag.base_positions.get(node_id).copied() else {
                continue;
            };
            let Some(auto) = drag.auto_positions.get(node_id).copied() else {
                continue;
            };
            let next = Point {
                x: snap_to_grid(base.x + drag.delta.x),
                y: snap_to_grid(base.y + drag.delta.y),
            };
            let value = if points_close(next, auto) {
                None
            } else {
                Some(next)
            };
            self.overrides.nodes.remove(node_id);
            if let Some(point) = value {
                self.overrides.nodes.insert(node_id.clone(), point);
            }
            update.nodes.insert(node_id.clone(), value);
        }
        Ok(Some(update))
    }

    pub fn begin_gantt_task_drag(&mut self, id: &str, mode: &str, pointer_x: f32) -> Result<()> {
        let drag_mode = parse_gantt_drag_mode(mode)?;
        let vm = self.view_model()?;
        let gantt = vm
            .gantt
            .ok_or_else(|| anyhow!("gantt data unavailable for drag"))?;
        let task = gantt
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or_else(|| anyhow!("gantt task '{id}' not found"))?;

        let pointer_day = gantt_x_to_day(pointer_x, &gantt);
        let grab_offset_day = match drag_mode {
            GanttDragMode::Move => pointer_day - task.start_day,
            GanttDragMode::ResizeStart => 0.0,
            GanttDragMode::ResizeEnd => 0.0,
            GanttDragMode::Milestone => pointer_day - task.start_day,
        };

        self.drag_state = Some(DragState::GanttTask(GanttTaskDragState {
            id: id.to_string(),
            mode: drag_mode,
            min_day: gantt.min_day,
            max_day: gantt.max_day.max(gantt.min_day + 0.001),
            section_label_width: gantt.section_label_width,
            timeline_width: gantt.timeline_width,
            grab_offset_day,
            start_day: task.start_day,
            end_day: task.end_day,
            moved: false,
        }));
        Ok(())
    }

    pub fn update_gantt_task_drag(&mut self, pointer_x: f32) -> Result<()> {
        let Some(DragState::GanttTask(mut drag)) = self.drag_state.clone() else {
            return Ok(());
        };
        let pointer_day = gantt_x_to_day_with_limits(
            pointer_x,
            drag.min_day,
            drag.max_day,
            drag.section_label_width,
            drag.timeline_width,
        );
        const MIN_SPAN: f64 = 0.001;

        let mut next_start = drag.start_day;
        let mut next_end = drag.end_day;
        match drag.mode {
            GanttDragMode::Move => {
                let span = (drag.end_day - drag.start_day).max(MIN_SPAN);
                next_start = pointer_day - drag.grab_offset_day;
                next_end = next_start + span;
            }
            GanttDragMode::ResizeStart => {
                next_start = pointer_day.min(drag.end_day - MIN_SPAN);
            }
            GanttDragMode::ResizeEnd => {
                next_end = pointer_day.max(drag.start_day + MIN_SPAN);
            }
            GanttDragMode::Milestone => {
                next_start = pointer_day - drag.grab_offset_day;
                next_end = next_start + MIN_SPAN;
            }
        }

        drag.moved = drag.moved
            || (next_start - drag.start_day).abs() > f64::EPSILON
            || (next_end - drag.end_day).abs() > f64::EPSILON;
        drag.start_day = next_start;
        drag.end_day = next_end;
        self.drag_state = Some(DragState::GanttTask(drag));
        Ok(())
    }

    pub fn end_gantt_task_drag(&mut self) -> Result<Option<LayoutUpdateInput>> {
        let Some(DragState::GanttTask(drag)) = self.drag_state.take() else {
            return Ok(None);
        };
        if !drag.moved {
            return Ok(None);
        }

        let mut update = LayoutUpdateInput::default();
        let entry = GanttTaskUpdateInput {
            start_day: Some(drag.start_day),
            end_day: Some(drag.end_day),
        };
        self.overrides.gantt.tasks.insert(
            drag.id.clone(),
            crate::GanttTaskOverride {
                start_day: entry.start_day,
                end_day: entry.end_day,
            },
        );
        update.gantt_tasks.insert(drag.id, Some(entry));
        Ok(Some(update))
    }

    pub fn cancel_drag(&mut self) {
        self.drag_state = None;
    }

    pub fn nudge_node(&mut self, id: &str, dx: f32, dy: f32) -> Result<LayoutUpdateInput> {
        let effective = self.effective_overrides();
        let diagram = Diagram::parse(&self.definition)?;
        let layout = diagram.layout(Some(&effective))?;
        let current = layout
            .final_positions
            .get(id)
            .copied()
            .ok_or_else(|| anyhow!("node '{id}' not found"))?;
        let auto = layout
            .auto_positions
            .get(id)
            .copied()
            .ok_or_else(|| anyhow!("auto position missing node '{id}'"))?;

        let next = Point {
            x: snap_to_grid(current.x + dx),
            y: snap_to_grid(current.y + dy),
        };
        let value = if points_close(next, auto) {
            None
        } else {
            Some(next)
        };

        self.overrides.nodes.remove(id);
        if let Some(point) = value {
            self.overrides.nodes.insert(id.to_string(), point);
        }

        let mut update = LayoutUpdateInput::default();
        update.nodes.insert(id.to_string(), value);
        Ok(update)
    }

    fn effective_overrides(&self) -> LayoutOverrides {
        let mut effective = self.overrides.clone();
        if let Some(drag) = &self.drag_state {
            match drag {
                DragState::Node(node) => {
                    effective.nodes.insert(node.id.clone(), node.current);
                }
                DragState::Edge(edge) => {
                    effective.edges.insert(
                        edge.id.clone(),
                        EdgeOverride {
                            points: edge.points.clone(),
                        },
                    );
                }
                DragState::Subgraph(subgraph) => {
                    for node_id in &subgraph.members {
                        if let Some(base) = subgraph.base_positions.get(node_id).copied() {
                            effective.nodes.insert(
                                node_id.clone(),
                                Point {
                                    x: base.x + subgraph.delta.x,
                                    y: base.y + subgraph.delta.y,
                                },
                            );
                        }
                    }
                }
                DragState::GanttTask(task) => {
                    effective.gantt.tasks.insert(
                        task.id.clone(),
                        crate::GanttTaskOverride {
                            start_day: Some(task.start_day),
                            end_day: Some(task.end_day),
                        },
                    );
                }
            }
        }
        effective
    }
}

fn snap_to_grid(value: f32) -> f32 {
    const GRID_SIZE: f32 = 10.0;
    (value / GRID_SIZE).round() * GRID_SIZE
}

fn points_close(a: Point, b: Point) -> bool {
    const EPSILON: f32 = 0.5;
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON
}

fn parse_gantt_drag_mode(mode: &str) -> Result<GanttDragMode> {
    match mode {
        "move" => Ok(GanttDragMode::Move),
        "resize-start" => Ok(GanttDragMode::ResizeStart),
        "resize-end" => Ok(GanttDragMode::ResizeEnd),
        "milestone" => Ok(GanttDragMode::Milestone),
        _ => Err(anyhow!("unsupported gantt drag mode '{mode}'")),
    }
}

fn gantt_x_to_day(x: f32, gantt: &GanttViewModel) -> f64 {
    gantt_x_to_day_with_limits(
        x,
        gantt.min_day,
        gantt.max_day,
        gantt.section_label_width,
        gantt.timeline_width,
    )
}

fn gantt_x_to_day_with_limits(
    x: f32,
    min_day: f64,
    max_day: f64,
    section_label_width: f32,
    timeline_width: f32,
) -> f64 {
    let clamped_x = x.clamp(
        section_label_width,
        section_label_width + timeline_width.max(1.0),
    );
    let ratio = ((clamped_x - section_label_width) / timeline_width.max(1.0)) as f64;
    min_day + (max_day - min_day) * ratio
}

fn merge_source_and_overrides(definition: &str, overrides: &LayoutOverrides) -> Result<String> {
    let trimmed = definition.trim_end_matches('\n');
    let mut output = trimmed.to_string();
    output.push('\n');

    if overrides.is_empty() {
        return Ok(output);
    }

    let json = serde_json::to_string_pretty(overrides)?;
    if json.trim() == "{}" {
        return Ok(output);
    }

    output.push('\n');
    output.push_str(crate::LAYOUT_BLOCK_START);
    output.push('\n');

    for line in json.lines() {
        output.push_str("%% ");
        output.push_str(line);
        output.push('\n');
    }

    output.push_str(crate::LAYOUT_BLOCK_END);
    output.push('\n');

    Ok(output)
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct WasmEditorCore {
        inner: RefCell<EditorCore>,
    }

    #[wasm_bindgen]
    impl WasmEditorCore {
        #[wasm_bindgen(constructor)]
        pub fn new(source: &str, background: &str) -> Result<WasmEditorCore, JsValue> {
            let core = EditorCore::from_source(source, background).map_err(to_js_error)?;
            Ok(WasmEditorCore {
                inner: RefCell::new(core),
            })
        }

        #[wasm_bindgen(js_name = viewModel)]
        pub fn view_model(&self) -> Result<JsValue, JsValue> {
            let vm = self.inner.borrow().view_model().map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&vm).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = renderSvg)]
        pub fn render_svg(&self) -> Result<String, JsValue> {
            self.inner.borrow().render_svg().map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = applyLayoutUpdate)]
        pub fn apply_layout_update(&self, update: JsValue) -> Result<(), JsValue> {
            let update: LayoutUpdateInput =
                serde_wasm_bindgen::from_value(update).map_err(to_js_error)?;
            self.inner.borrow_mut().apply_layout_update(update);
            Ok(())
        }

        #[wasm_bindgen(js_name = applyStyleUpdate)]
        pub fn apply_style_update(&self, update: JsValue) -> Result<(), JsValue> {
            let update: StyleUpdateInput =
                serde_wasm_bindgen::from_value(update).map_err(to_js_error)?;
            self.inner.borrow_mut().apply_style_update(update);
            Ok(())
        }

        #[wasm_bindgen(js_name = setSource)]
        pub fn set_source(&self, source: &str) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .set_source(source)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = setBackground)]
        pub fn set_background(&self, background: &str) {
            self.inner
                .borrow_mut()
                .set_background(background.to_string());
        }

        #[wasm_bindgen(js_name = beginNodeDrag)]
        pub fn begin_node_drag(
            &self,
            id: &str,
            pointer_x: f32,
            pointer_y: f32,
        ) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .begin_node_drag(id, pointer_x, pointer_y)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = updateNodeDrag)]
        pub fn update_node_drag(&self, pointer_x: f32, pointer_y: f32) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .update_node_drag(pointer_x, pointer_y)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = endNodeDrag)]
        pub fn end_node_drag(&self) -> Result<JsValue, JsValue> {
            let update = self
                .inner
                .borrow_mut()
                .end_node_drag()
                .map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&update).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = beginEdgeDrag)]
        pub fn begin_edge_drag(&self, id: &str, index: usize) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .begin_edge_drag(id, index)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = updateEdgeDrag)]
        pub fn update_edge_drag(&self, pointer_x: f32, pointer_y: f32) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .update_edge_drag(pointer_x, pointer_y)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = endEdgeDrag)]
        pub fn end_edge_drag(&self) -> Result<JsValue, JsValue> {
            let update = self
                .inner
                .borrow_mut()
                .end_edge_drag()
                .map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&update).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = beginSubgraphDrag)]
        pub fn begin_subgraph_drag(
            &self,
            id: &str,
            pointer_x: f32,
            pointer_y: f32,
        ) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .begin_subgraph_drag(id, pointer_x, pointer_y)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = updateSubgraphDrag)]
        pub fn update_subgraph_drag(&self, pointer_x: f32, pointer_y: f32) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .update_subgraph_drag(pointer_x, pointer_y)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = endSubgraphDrag)]
        pub fn end_subgraph_drag(&self) -> Result<JsValue, JsValue> {
            let update = self
                .inner
                .borrow_mut()
                .end_subgraph_drag()
                .map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&update).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = beginGanttTaskDrag)]
        pub fn begin_gantt_task_drag(
            &self,
            id: &str,
            mode: &str,
            pointer_x: f32,
        ) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .begin_gantt_task_drag(id, mode, pointer_x)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = updateGanttTaskDrag)]
        pub fn update_gantt_task_drag(&self, pointer_x: f32) -> Result<(), JsValue> {
            self.inner
                .borrow_mut()
                .update_gantt_task_drag(pointer_x)
                .map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = endGanttTaskDrag)]
        pub fn end_gantt_task_drag(&self) -> Result<JsValue, JsValue> {
            let update = self
                .inner
                .borrow_mut()
                .end_gantt_task_drag()
                .map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&update).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = cancelDrag)]
        pub fn cancel_drag(&self) {
            self.inner.borrow_mut().cancel_drag();
        }

        #[wasm_bindgen(js_name = nudgeNode)]
        pub fn nudge_node(&self, id: &str, dx: f32, dy: f32) -> Result<JsValue, JsValue> {
            let update = self
                .inner
                .borrow_mut()
                .nudge_node(id, dx, dy)
                .map_err(to_js_error)?;
            serde_wasm_bindgen::to_value(&update).map_err(to_js_error)
        }

        #[wasm_bindgen(js_name = source)]
        pub fn source(&self) -> Result<String, JsValue> {
            self.inner.borrow().source().map_err(to_js_error)
        }
    }

    fn to_js_error<E: std::fmt::Display>(err: E) -> JsValue {
        JsValue::from_str(&err.to_string())
    }
}
