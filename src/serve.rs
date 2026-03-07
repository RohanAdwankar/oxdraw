use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceExt;
use tower::service_fn;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use walkdir::WalkDir;

use crate::codemap::CodeMapMapping;
use crate::diagram::decode_image_dimensions;
use crate::*;

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_IMAGE_REQUEST_BYTES: usize = (MAX_IMAGE_BYTES * 4) / 3 + 1 * 1024 * 1024;

/// Arguments for running the oxdraw web server
#[derive(Debug, Clone, Parser)]
#[command(name = "oxdraw serve", about = "Start the oxdraw web sync API server.")]
pub struct ServeArgs {
    /// Path to the diagram definition that should be served.
    #[arg(short = 'i', long = "input")]
    pub input: PathBuf,

    /// Address to bind the HTTP server to.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(long, default_value_t = 5151)]
    pub port: u16,

    /// Background color for rendered SVG previews.
    #[arg(long = "background-color", default_value = "white")]
    pub background_color: String,

    /// Path to the codebase for code map mode.
    #[clap(skip)]
    pub code_map_root: Option<PathBuf>,

    /// Mapping data for code map mode.
    #[clap(skip)]
    pub code_map_mapping: Option<CodeMapMapping>,

    /// Warning message if the code map is out of sync.
    #[clap(skip)]
    pub code_map_warning: Option<String>,
}

struct ServeState {
    source_path: PathBuf,
    background: String,
    overrides: RwLock<LayoutOverrides>,
    source_lock: Mutex<()>,
    code_map_root: Option<PathBuf>,
    code_map_mapping: Option<CodeMapMapping>,
    code_map_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagramPayload {
    source_path: String,
    kind: String,
    background: String,
    auto_size: CanvasSize,
    render_size: CanvasSize,
    nodes: Vec<NodePayload>,
    edges: Vec<EdgePayload>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    subgraphs: Vec<SubgraphPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gantt: Option<GanttPayload>,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GanttPayload {
    date_format: String,
    title: Option<String>,
    min_day: f64,
    max_day: f64,
    section_label_width: f32,
    timeline_width: f32,
    top_margin: f32,
    row_height: f32,
    bar_height: f32,
    right_padding: f32,
    bottom_margin: f32,
    sections: Vec<String>,
    tasks: Vec<GanttTaskPayload>,
    style: GanttStylePayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GanttTaskPayload {
    id: String,
    label: String,
    section_index: usize,
    row_index: usize,
    start_day: f64,
    end_day: f64,
    milestone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GanttStylePayload {
    row_fill_even: String,
    row_fill_odd: String,
    task_fill: String,
    milestone_fill: String,
    task_text: String,
    milestone_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodePayload {
    id: String,
    label: String,
    shape: String,
    auto_position: Point,
    rendered_position: Point,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label_fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_fill_color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    membership: Vec<String>,
    width: f32,
    height: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<NodeImagePayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeImagePayload {
    mime_type: String,
    data: String,
    width: u32,
    height: u32,
    padding: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubgraphPayload {
    id: String,
    label: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    label_x: f32,
    label_y: f32,
    depth: usize,
    order: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EdgePayload {
    id: String,
    from: String,
    to: String,
    label: Option<String>,
    kind: String,
    auto_points: Vec<Point>,
    rendered_points: Vec<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_points: Option<Vec<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrow_direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    marker_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    marker_end: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LayoutUpdate {
    #[serde(default)]
    nodes: HashMap<String, Option<Point>>,
    #[serde(default)]
    edges: HashMap<String, Option<EdgeOverride>>,
    #[serde(default)]
    gantt_tasks: HashMap<String, Option<GanttTaskLayoutUpdate>>,
}

#[derive(Debug, Deserialize, Default)]
struct GanttTaskLayoutUpdate {
    #[serde(default)]
    start_day: Option<f64>,
    #[serde(default)]
    end_day: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SourceUpdateRequest {
    source: String,
}

#[derive(Debug, Deserialize, Default)]
struct StyleUpdate {
    #[serde(default)]
    node_styles: HashMap<String, Option<NodeStylePatch>>,
    #[serde(default)]
    edge_styles: HashMap<String, Option<EdgeStylePatch>>,
    #[serde(default)]
    gantt_style: Option<GanttStylePatch>,
}

#[derive(Debug, Deserialize)]
struct NodeImageUpdateRequest {
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    padding: Option<f32>,
}

impl ServeState {
    async fn read_diagram(&self) -> Result<(String, Diagram)> {
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let diagram = match Diagram::parse(&definition) {
            Ok(d) => d,
            Err(e) => {
                // If this is a markdown file and we failed to parse as a diagram,
                // return a dummy diagram so the UI can load and switch to codedown mode.
                let is_md = self
                    .source_path
                    .extension()
                    .map_or(false, |ext| ext == "md");
                if is_md {
                    let mut nodes = HashMap::new();
                    nodes.insert(
                        "dummy".to_string(),
                        Node {
                            label: "Loading...".to_string(),
                            shape: NodeShape::Rectangle,
                            image: None,
                            width: NODE_WIDTH,
                            height: NODE_HEIGHT,
                        },
                    );
                    Diagram {
                        kind: DiagramKind::Flowchart,
                        direction: Direction::TopDown,
                        nodes,
                        order: vec!["dummy".to_string()],
                        edges: Vec::new(),
                        subgraphs: Vec::new(),
                        node_membership: HashMap::new(),
                    }
                } else {
                    return Err(e);
                }
            }
        };
        Ok((contents, diagram))
    }

    async fn current_overrides(&self) -> LayoutOverrides {
        self.overrides.read().await.clone()
    }

    async fn apply_update(&self, update: LayoutUpdate) -> Result<()> {
        let LayoutUpdate {
            nodes,
            edges,
            gantt_tasks,
        } = update;

        if !gantt_tasks.is_empty() {
            self.apply_gantt_task_updates(&gantt_tasks).await?;
        }

        if nodes.is_empty() && edges.is_empty() {
            return Ok(());
        }

        let snapshot = {
            let mut overrides = self.overrides.write().await;
            let mut changed = false;

            for (id, value) in nodes {
                match value {
                    Some(point) => {
                        overrides.nodes.insert(id, point);
                        changed = true;
                    }
                    None => {
                        if overrides.nodes.remove(&id).is_some() {
                            changed = true;
                        }
                    }
                }
            }

            for (id, value) in edges {
                match value {
                    Some(edge_override) if !edge_override.points.is_empty() => {
                        overrides.edges.insert(id, edge_override);
                        changed = true;
                    }
                    _ => {
                        if overrides.edges.remove(&id).is_some() {
                            changed = true;
                        }
                    }
                }
            }

            if !changed {
                None
            } else {
                Some(overrides.clone())
            }
        };

        if let Some(snapshot) = snapshot {
            self.rewrite_file_with_overrides(&snapshot).await
        } else {
            Ok(())
        }
    }

    async fn apply_gantt_task_updates(
        &self,
        gantt_tasks: &HashMap<String, Option<GanttTaskLayoutUpdate>>,
    ) -> Result<()> {
        let _guard = self.source_lock.lock().await;
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let diagram = Diagram::parse(&definition)?;
        let DiagramKind::Gantt(gantt) = &diagram.kind else {
            return Ok(());
        };

        let rewritten = rewrite_gantt_task_lines(&definition, gantt, gantt_tasks);

        let snapshot = {
            let mut overrides = self.overrides.write().await;
            for id in gantt_tasks.keys() {
                overrides.gantt.tasks.remove(id);
                overrides.nodes.remove(id);
            }
            overrides.clone()
        };

        let merged = merge_source_and_overrides(&rewritten, &snapshot)?;
        tokio::fs::write(&self.source_path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
        Ok(())
    }

    async fn apply_style_update(&self, update: StyleUpdate) -> Result<()> {
        let snapshot = {
            let mut overrides = self.overrides.write().await;

            for (id, value) in update.node_styles {
                match value {
                    Some(patch) => {
                        let mut current = overrides.node_styles.remove(&id).unwrap_or_default();

                        if let Some(fill) = patch.fill {
                            current.fill = fill;
                        }
                        if let Some(stroke) = patch.stroke {
                            current.stroke = stroke;
                        }
                        if let Some(text) = patch.text {
                            current.text = text;
                        }
                        if let Some(label_fill) = patch.label_fill {
                            current.label_fill = label_fill;
                        }
                        if let Some(image_fill) = patch.image_fill {
                            current.image_fill = image_fill;
                        }

                        if current.is_empty() {
                            overrides.node_styles.remove(&id);
                        } else {
                            overrides.node_styles.insert(id, current);
                        }
                    }
                    None => {
                        overrides.node_styles.remove(&id);
                    }
                }
            }

            for (id, value) in update.edge_styles {
                match value {
                    Some(patch) => {
                        let mut current = overrides.edge_styles.remove(&id).unwrap_or_default();

                        if let Some(line) = patch.line {
                            current.line = line;
                        }
                        if let Some(color) = patch.color {
                            current.color = color;
                        }
                        if let Some(arrow) = patch.arrow {
                            current.arrow = arrow;
                        }

                        if current.is_empty() {
                            overrides.edge_styles.remove(&id);
                        } else {
                            overrides.edge_styles.insert(id, current);
                        }
                    }
                    None => {
                        overrides.edge_styles.remove(&id);
                    }
                }
            }

            if let Some(patch) = update.gantt_style {
                if let Some(value) = patch.row_fill_even {
                    overrides.gantt.style.row_fill_even = value;
                }
                if let Some(value) = patch.row_fill_odd {
                    overrides.gantt.style.row_fill_odd = value;
                }
                if let Some(value) = patch.task_fill {
                    overrides.gantt.style.task_fill = value;
                }
                if let Some(value) = patch.milestone_fill {
                    overrides.gantt.style.milestone_fill = value;
                }
                if let Some(value) = patch.milestone_text {
                    overrides.gantt.style.milestone_text = value;
                }
                if let Some(value) = patch.task_text {
                    overrides.gantt.style.task_text = value;
                }
            }

            overrides.clone()
        };

        self.rewrite_file_with_overrides(&snapshot).await
    }

    async fn prune_overrides_for(&self, diagram: &Diagram) -> Result<()> {
        let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
        let edge_ids: HashSet<String> = diagram
            .edges
            .iter()
            .map(|edge| edge_identifier(edge))
            .collect();

        let snapshot = {
            let mut overrides = self.overrides.write().await;
            overrides.prune(&node_ids, &edge_ids);
            overrides.clone()
        };

        let definition = diagram.to_definition();
        self.write_definition_with_overrides(&definition, &snapshot)
            .await
    }

    async fn replace_source(&self, contents: &str) -> Result<()> {
        let has_block = contents
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case(LAYOUT_BLOCK_START));
        let (definition, parsed_overrides) = split_source_and_overrides(contents)?;
        let diagram = Diagram::parse(&definition)?;

        let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
        let edge_ids: HashSet<String> = diagram
            .edges
            .iter()
            .map(|edge| edge_identifier(edge))
            .collect();

        let snapshot = {
            let mut overrides = self.overrides.write().await;
            if has_block {
                *overrides = parsed_overrides;
            }
            overrides.prune(&node_ids, &edge_ids);
            overrides.clone()
        };

        self.write_definition_with_overrides(&definition, &snapshot)
            .await
    }

    async fn rewrite_file_with_overrides(&self, overrides: &LayoutOverrides) -> Result<()> {
        let _guard = self.source_lock.lock().await;
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let merged = merge_source_and_overrides(&definition, overrides)?;
        tokio::fs::write(&self.source_path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
        Ok(())
    }

    async fn write_definition_with_overrides(
        &self,
        definition: &str,
        overrides: &LayoutOverrides,
    ) -> Result<()> {
        let merged = merge_source_and_overrides(definition, overrides)?;
        let _guard = self.source_lock.lock().await;
        tokio::fs::write(&self.source_path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
        Ok(())
    }

    async fn remove_node(&self, node_id: &str) -> Result<bool> {
        let diagram = {
            let _guard = self.source_lock.lock().await;
            let source = tokio::fs::read_to_string(&self.source_path)
                .await
                .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
            let mut diagram = Diagram::parse(&source)?;
            if diagram.nodes.len() == 1 && diagram.nodes.contains_key(node_id) {
                bail!("diagram must contain at least one node");
            }
            if !diagram.remove_node(node_id) {
                return Ok(false);
            }
            let rewritten = diagram.to_definition();
            tokio::fs::write(&self.source_path, rewritten.as_bytes())
                .await
                .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
            diagram
        };

        self.prune_overrides_for(&diagram).await?;
        Ok(true)
    }

    async fn remove_edge(&self, edge_id: &str) -> Result<bool> {
        let diagram = {
            let _guard = self.source_lock.lock().await;
            let source = tokio::fs::read_to_string(&self.source_path)
                .await
                .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
            let mut diagram = Diagram::parse(&source)?;
            if !diagram.remove_edge_by_identifier(edge_id) {
                return Ok(false);
            }
            let rewritten = diagram.to_definition();
            tokio::fs::write(&self.source_path, rewritten.as_bytes())
                .await
                .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
            diagram
        };

        self.prune_overrides_for(&diagram).await?;
        Ok(true)
    }

    async fn set_node_image(&self, node_id: &str, image: Option<NodeImage>) -> Result<()> {
        let overrides_snapshot = self.overrides.read().await.clone();
        let _guard = self.source_lock.lock().await;
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let mut diagram = Diagram::parse(&definition)?;
        let Some(node) = diagram.nodes.get_mut(node_id) else {
            bail!("node '{node_id}' not found");
        };
        node.image = image;
        let rewritten = diagram.to_definition();
        let merged = merge_source_and_overrides(&rewritten, &overrides_snapshot)?;
        tokio::fs::write(&self.source_path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
        Ok(())
    }

    async fn update_node_image_padding(&self, node_id: &str, padding: f32) -> Result<()> {
        let overrides_snapshot = self.overrides.read().await.clone();
        let _guard = self.source_lock.lock().await;
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let mut diagram = Diagram::parse(&definition)?;
        let Some(node) = diagram.nodes.get_mut(node_id) else {
            bail!("node '{node_id}' not found");
        };
        let Some(image) = node.image.as_mut() else {
            bail!("node '{node_id}' does not have an image to update");
        };
        image.padding = padding;
        let rewritten = diagram.to_definition();
        let merged = merge_source_and_overrides(&rewritten, &overrides_snapshot)?;
        tokio::fs::write(&self.source_path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", self.source_path.display()))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct OpenRequest {
    path: String,
    line: Option<usize>,
    editor: String,
}

async fn open_in_editor(
    State(state): State<Arc<ServeState>>,
    Json(payload): Json<OpenRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let root = state.code_map_root.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "Code map mode not active".to_string(),
    ))?;
    let full_path = root.join(&payload.path);

    if !full_path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found".to_string()));
    }

    let line = payload.line.unwrap_or(1);

    let result = match payload.editor.as_str() {
        "vscode" => std::process::Command::new("code")
            .arg("-g")
            .arg(format!("{}:{}", full_path.display(), line))
            .spawn(),
        "nvim" => {
            // On macOS, we need to open a new terminal window for nvim
            #[cfg(target_os = "macos")]
            {
                let cmd = format!("cd {:?} && vi +{} {:?}", root, line, payload.path);
                let escaped_cmd = cmd.replace("\\", "\\\\").replace("\"", "\\\"");

                std::process::Command::new("osascript")
                    .arg("-e")
                    .arg(format!(
                        "tell application \"Terminal\" to do script \"{}\"",
                        escaped_cmd
                    ))
                    .spawn()
            }
            #[cfg(not(target_os = "macos"))]
            {
                // Fallback for other OSs - this might still fail if not in a GUI environment
                // or if the server is headless.
                std::process::Command::new("vi")
                    .current_dir(root)
                    .arg(format!("+{}", line))
                    .arg(&payload.path)
                    .spawn()
            }
        }
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown editor".to_string())),
    };

    match result {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to launch editor: {}", e),
        )),
    }
}

pub async fn run_serve(args: ServeArgs, ui_root: Option<PathBuf>) -> Result<()> {
    let initial_source = fs::read_to_string(&args.input)
        .with_context(|| format!("failed to read '{}'", args.input.display()))?;
    let (_, overrides) = split_source_and_overrides(&initial_source)?;

    let state = Arc::new(ServeState {
        source_path: args.input.clone(),
        background: args.background_color.clone(),
        overrides: RwLock::new(overrides),
        source_lock: Mutex::new(()),
        code_map_root: args.code_map_root,
        code_map_mapping: args.code_map_mapping,
        code_map_warning: args.code_map_warning,
    });

    let mut app = Router::new()
        .route("/api/diagram", get(get_diagram))
        .route("/api/diagram/svg", get(get_svg))
        .route("/api/diagram/layout", put(put_layout))
        .route("/api/diagram/style", put(put_style))
        .route("/api/diagram/source", get(get_source).put(put_source))
        .route("/api/diagram/nodes/:id/image", put(put_node_image))
        .route("/api/diagram/nodes/:id", delete(delete_node))
        .route("/api/diagram/edges/:id", delete(delete_edge))
        .route("/api/codemap/mapping", get(get_codemap_mapping))
        .route("/api/codemap/status", get(get_codemap_status))
        .route("/api/codemap/file", get(get_codemap_file))
        .route("/api/codemap/search", get(get_codemap_search))
        .route("/api/codemap/open", axum::routing::post(open_in_editor))
        .layer(DefaultBodyLimit::max(MAX_IMAGE_REQUEST_BYTES))
        .with_state(state);

    if let Some(root) = ui_root {
        let static_dir = ServeDir::new(root.clone())
            .append_index_html_on_directories(true)
            .fallback(ServeFile::new(root.join("index.html")));
        let dir_for_service = static_dir.clone();

        let static_service = service_fn(move |req| {
            let svc = dir_for_service.clone();
            async move {
                match svc.oneshot(req).await {
                    Ok(response) => Ok(response.map(axum::body::Body::new)),
                    Err(error) => {
                        let message = format!("Static file error: {error}");
                        Ok((StatusCode::INTERNAL_SERVER_ERROR, message).into_response())
                    }
                }
            }
        });

        app = app.fallback_service(static_service);
    }

    let app = app.layer(CorsLayer::permissive());

    let addr = format!("{}:{}", args.host, args.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind HTTP server to {addr}"))?;

    println!("oxdraw server listening on http://{addr}");
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("HTTP server error")?;

    Ok(())
}

async fn get_diagram(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let (source, diagram) = state.read_diagram().await.map_err(internal_error)?;
    let overrides = state.current_overrides().await;

    let layout = diagram.layout(Some(&overrides)).map_err(internal_error)?;
    let geometry = align_geometry(
        &layout.final_positions,
        &layout.final_routes,
        &diagram.edges,
        &diagram.subgraphs,
        &diagram.nodes,
    )
    .map_err(internal_error)?;

    let mut nodes = Vec::new();
    for id in &diagram.order {
        let node = diagram
            .nodes
            .get(id)
            .ok_or_else(|| internal_error(anyhow!("node '{id}' missing from diagram")))?;
        let auto_position = layout
            .auto_positions
            .get(id)
            .copied()
            .ok_or_else(|| internal_error(anyhow!("auto layout missing node '{id}'")))?;
        let final_position = layout
            .final_positions
            .get(id)
            .copied()
            .ok_or_else(|| internal_error(anyhow!("final layout missing node '{id}'")))?;
        let override_position = overrides.nodes.get(id).copied();
        let style = overrides.node_styles.get(id);
        let fill_color = style.and_then(|s| s.fill.clone());
        let stroke_color = style.and_then(|s| s.stroke.clone());
        let text_color = style.and_then(|s| s.text.clone());
        let label_fill_color = style.and_then(|s| s.label_fill.clone());
        let image_fill_color = style.and_then(|s| s.image_fill.clone());
        let image_payload = node.image.as_ref().map(|image| NodeImagePayload {
            mime_type: image.mime_type.clone(),
            data: BASE64_STANDARD.encode(&image.data),
            width: image.width,
            height: image.height,
            padding: image.padding.max(0.0),
        });
        nodes.push(NodePayload {
            id: id.clone(),
            label: node.label.clone(),
            shape: node.shape.as_str().to_string(),
            auto_position,
            rendered_position: final_position,
            override_position,
            fill_color,
            stroke_color,
            text_color,
            label_fill_color,
            image_fill_color,
            membership: diagram.node_membership.get(id).cloned().unwrap_or_default(),
            width: node.width,
            height: node.height,
            image: image_payload,
        });
    }

    let mut edges = Vec::new();
    for edge in &diagram.edges {
        let identifier = edge_identifier(edge);
        let auto_points = layout
            .auto_routes
            .get(&identifier)
            .cloned()
            .unwrap_or_default();
        let final_points = layout
            .final_routes
            .get(&identifier)
            .cloned()
            .unwrap_or_default();
        let manual_points = overrides
            .edges
            .get(&identifier)
            .map(|edge_override| edge_override.points.clone());
        let style = overrides.edge_styles.get(&identifier);
        let line_kind = style
            .and_then(|s| s.line)
            .unwrap_or(edge.kind)
            .as_str()
            .to_string();
        let color = style.and_then(|s| s.color.clone());
        let arrow_direction = style
            .and_then(|s| s.arrow)
            .map(|direction| direction.as_str().to_string());
        let marker_start = if edge.marker_start == EdgeEndpointMarker::None {
            None
        } else {
            Some(edge.marker_start.as_str().to_string())
        };
        let marker_end = if edge.marker_end == EdgeEndpointMarker::None {
            None
        } else {
            Some(edge.marker_end.as_str().to_string())
        };

        edges.push(EdgePayload {
            id: identifier,
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            kind: line_kind,
            auto_points,
            rendered_points: final_points,
            override_points: manual_points,
            color,
            arrow_direction,
            marker_start,
            marker_end,
        });
    }

    let mut subgraphs = Vec::new();
    for sg in &geometry.subgraphs {
        subgraphs.push(SubgraphPayload {
            id: sg.id.clone(),
            label: sg.label.clone(),
            x: sg.x,
            y: sg.y,
            width: sg.width,
            height: sg.height,
            label_x: sg.label_x,
            label_y: sg.label_y,
            depth: sg.depth,
            order: sg.order,
            parent_id: sg.parent_id.clone(),
        });
    }

    let (kind, gantt_payload) = match &diagram.kind {
        DiagramKind::Flowchart => ("flowchart".to_string(), None),
        DiagramKind::Gantt(gantt) => {
            let gantt_overrides = &overrides.gantt;
            let row_fill_even = gantt_overrides
                .style
                .row_fill_even
                .clone()
                .unwrap_or_else(|| "#eff6ff".to_string());
            let row_fill_odd = gantt_overrides
                .style
                .row_fill_odd
                .clone()
                .unwrap_or_else(|| "#dbeafe".to_string());
            let task_fill = gantt_overrides
                .style
                .task_fill
                .clone()
                .unwrap_or_else(|| "#2563eb".to_string());
            let milestone_fill = gantt_overrides
                .style
                .milestone_fill
                .clone()
                .unwrap_or_else(|| "#1d4ed8".to_string());
            let task_text = gantt_overrides
                .style
                .task_text
                .clone()
                .unwrap_or_else(|| "#ffffff".to_string());
            let milestone_text = gantt_overrides
                .style
                .milestone_text
                .clone()
                .unwrap_or_else(|| "#111827".to_string());

            let mut min_day = f64::INFINITY;
            let mut max_day = f64::NEG_INFINITY;
            let mut tasks = Vec::with_capacity(gantt.tasks.len());

            for (row_index, task) in gantt.tasks.iter().enumerate() {
                let task_override = gantt_overrides.tasks.get(&task.id);
                let start_day = task_override
                    .and_then(|entry| entry.start_day)
                    .unwrap_or(task.start_day);
                let mut end_day = task_override
                    .and_then(|entry| entry.end_day)
                    .unwrap_or(task.end_day);
                if end_day <= start_day {
                    end_day = start_day + 0.001;
                }

                min_day = min_day.min(start_day);
                max_day = max_day.max(end_day);

                tasks.push(GanttTaskPayload {
                    id: task.id.clone(),
                    label: task.label.clone(),
                    section_index: task.section_index,
                    row_index,
                    start_day,
                    end_day,
                    milestone: task.milestone,
                });
            }

            if !min_day.is_finite() || !max_day.is_finite() || max_day <= min_day {
                min_day = 0.0;
                max_day = 1.0;
            }

            (
                "gantt".to_string(),
                Some(GanttPayload {
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
                    style: GanttStylePayload {
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

    let payload = DiagramPayload {
        source_path: state.source_path.display().to_string(),
        kind,
        background: state.background.clone(),
        auto_size: layout.auto_size,
        render_size: CanvasSize {
            width: geometry.width,
            height: geometry.height,
        },
        nodes,
        edges,
        subgraphs,
        gantt: gantt_payload,
        source,
    };

    Ok(Json(payload))
}

async fn get_svg(State(state): State<Arc<ServeState>>) -> Result<Response, (StatusCode, String)> {
    let (_, diagram) = state.read_diagram().await.map_err(internal_error)?;
    let overrides = state.current_overrides().await;
    let override_ref = if overrides.is_empty() {
        None
    } else {
        Some(&overrides)
    };

    let svg = diagram
        .render_svg(&state.background, override_ref)
        .map_err(internal_error)?;

    let mut response = Response::new(svg.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"),
    );
    Ok(response)
}

async fn put_layout(
    State(state): State<Arc<ServeState>>,
    Json(update): Json<LayoutUpdate>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state.apply_update(update).await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_style(
    State(state): State<Arc<ServeState>>,
    Json(update): Json<StyleUpdate>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .apply_style_update(update)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_source(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<SourcePayload>, (StatusCode, String)> {
    let (source, _) = state.read_diagram().await.map_err(internal_error)?;
    Ok(Json(SourcePayload { source }))
}

async fn put_source(
    State(state): State<Arc<ServeState>>,
    Json(payload): Json<SourceUpdateRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .replace_source(&payload.source)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_node(
    State(state): State<Arc<ServeState>>,
    AxumPath(node_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.remove_node(&node_id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, format!("node '{node_id}' not found"))),
        Err(err) => {
            let message = err.to_string();
            if message.contains("at least one node") {
                Err((StatusCode::BAD_REQUEST, message))
            } else {
                Err(internal_error(err))
            }
        }
    }
}

async fn delete_edge(
    State(state): State<Arc<ServeState>>,
    AxumPath(edge_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.remove_edge(&edge_id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err((StatusCode::NOT_FOUND, format!("edge '{edge_id}' not found"))),
        Err(err) => Err(internal_error(err)),
    }
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn rewrite_gantt_task_lines(
    definition: &str,
    gantt: &GanttData,
    gantt_tasks: &HashMap<String, Option<GanttTaskLayoutUpdate>>,
) -> String {
    if gantt_tasks.is_empty() {
        return definition.to_string();
    }

    let mut lines: Vec<String> = definition.lines().map(ToString::to_string).collect();
    let mut task_by_id: HashMap<&str, &GanttTask> = HashMap::new();
    let mut task_index_by_id: HashMap<&str, usize> = HashMap::new();
    for task in &gantt.tasks {
        task_by_id.insert(task.id.as_str(), task);
    }
    for (idx, task) in gantt.tasks.iter().enumerate() {
        task_index_by_id.insert(task.id.as_str(), idx);
    }

    let task_line_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("section ") {
                return None;
            }
            if trimmed.trim_end_matches(';').contains(':') {
                Some(idx)
            } else {
                None
            }
        })
        .collect();

    for (task_id, patch_opt) in gantt_tasks {
        let Some(patch) = patch_opt else {
            continue;
        };
        let Some(task) = task_by_id.get(task_id.as_str()) else {
            continue;
        };

        let start_day = patch.start_day.unwrap_or(task.start_day);
        let mut end_day = patch.end_day.unwrap_or(task.end_day);
        if end_day < start_day {
            end_day = start_day;
        }

        let start_text = format_gantt_day(start_day, &gantt.date_format);
        let end_text = if task.milestone {
            "0d".to_string()
        } else {
            format_gantt_day(end_day, &gantt.date_format)
        };

        let mut replaced = false;
        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("section ") {
                continue;
            }

            let had_semicolon = trimmed.ends_with(';');
            let normalized = trimmed.trim_end_matches(';').trim();
            let Some((title_part, metadata_part)) = normalized.split_once(':') else {
                continue;
            };

            let metadata_tokens: Vec<String> = metadata_part
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToString::to_string)
                .collect();

            if !metadata_tokens.iter().any(|token| token == task_id) {
                continue;
            }

            let mut tags = Vec::new();
            let mut idx = 0_usize;
            while idx < metadata_tokens.len() {
                let lower = metadata_tokens[idx].to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "active" | "done" | "crit" | "milestone" | "milestore" | "vert"
                ) {
                    tags.push(metadata_tokens[idx].clone());
                    idx += 1;
                } else {
                    break;
                }
            }

            let mut new_meta = Vec::new();
            new_meta.extend(tags);
            new_meta.push(task_id.to_string());
            new_meta.push(start_text.clone());
            new_meta.push(end_text.clone());

            let indent_len = line.len().saturating_sub(line.trim_start().len());
            let indent = &line[..indent_len];
            let mut rebuilt = format!("{indent}{}: {}", title_part.trim_end(), new_meta.join(", "));
            if had_semicolon {
                rebuilt.push(';');
            }
            *line = rebuilt;
            replaced = true;
            break;
        }

        if !replaced {
            let Some(task_index) = task_index_by_id.get(task_id.as_str()).copied() else {
                continue;
            };
            let Some(line_index) = task_line_indices.get(task_index).copied() else {
                continue;
            };
            let original = &lines[line_index];
            let trimmed = original.trim();
            let had_semicolon = trimmed.ends_with(';');
            let normalized = trimmed.trim_end_matches(';').trim();
            let Some((title_part, metadata_part)) = normalized.split_once(':') else {
                continue;
            };
            let metadata_tokens: Vec<String> = metadata_part
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToString::to_string)
                .collect();

            let mut tags = Vec::new();
            let mut idx = 0_usize;
            while idx < metadata_tokens.len() {
                let lower = metadata_tokens[idx].to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "active" | "done" | "crit" | "milestone" | "milestore" | "vert"
                ) {
                    tags.push(metadata_tokens[idx].clone());
                    idx += 1;
                } else {
                    break;
                }
            }

            let mut new_meta = Vec::new();
            new_meta.extend(tags);
            if !task_id.starts_with("task_") {
                new_meta.push(task_id.to_string());
            }
            new_meta.push(start_text.clone());
            new_meta.push(end_text.clone());

            let indent_len = original.len().saturating_sub(original.trim_start().len());
            let indent = &original[..indent_len];
            let mut rebuilt = format!("{indent}{}: {}", title_part.trim_end(), new_meta.join(", "));
            if had_semicolon {
                rebuilt.push(';');
            }
            lines[line_index] = rebuilt;
        }
    }

    let mut output = lines.join("\n");
    if definition.ends_with('\n') {
        output.push('\n');
    }
    output
}

async fn put_node_image(
    State(state): State<Arc<ServeState>>,
    AxumPath(node_id): AxumPath<String>,
    Json(payload): Json<NodeImageUpdateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let NodeImageUpdateRequest {
        mime_type,
        data,
        padding,
    } = payload;

    let sanitized_padding = padding.map(|value| {
        if value.is_nan() || !value.is_finite() || value < 0.0 {
            0.0
        } else {
            value
        }
    });

    let data_str = match data
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => {
            if let Some(padding_value) = sanitized_padding {
                state
                    .update_node_image_padding(&node_id, padding_value)
                    .await
                    .map_err(internal_error)?;
            } else {
                state
                    .set_node_image(&node_id, None)
                    .await
                    .map_err(internal_error)?;
            }
            return Ok(StatusCode::NO_CONTENT);
        }
    };

    let mime_type = mime_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "mime_type is required when providing image data".to_string(),
            )
        })?
        .to_string();

    let data = BASE64_STANDARD.decode(data_str.as_bytes()).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid base64 payload: {err}"),
        )
    })?;

    if data.len() > MAX_IMAGE_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("image payload too large (max {} bytes)", MAX_IMAGE_BYTES),
        ));
    }

    let (width, height) = decode_image_dimensions(&mime_type, &data).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("unsupported image payload: {err}"),
        )
    })?;

    let image = NodeImage {
        mime_type,
        data,
        width,
        height,
        padding: sanitized_padding.unwrap_or(0.0),
    };

    state
        .set_node_image(&node_id, Some(image))
        .await
        .map_err(internal_error)?;

    Ok(StatusCode::NO_CONTENT)
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
    output.push_str(LAYOUT_BLOCK_START);
    output.push('\n');

    for line in json.lines() {
        output.push_str("%% ");
        output.push_str(line);
        output.push('\n');
    }

    output.push_str(LAYOUT_BLOCK_END);
    output.push('\n');

    Ok(output)
}

#[derive(Debug, Serialize)]
struct SourcePayload {
    source: String,
}

#[derive(Debug, Serialize)]
struct CodeMapStatus {
    warning: Option<String>,
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

#[derive(Debug, Deserialize)]
struct FileRequest {
    path: String,
}

async fn get_codemap_mapping(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<Option<CodeMapMapping>>, (StatusCode, String)> {
    let mapping = state.code_map_mapping.clone();
    let root = state.code_map_root.clone();

    if let (Some(mut mapping), Some(root)) = (mapping, root) {
        let mapping = tokio::task::spawn_blocking(move || {
            mapping.resolve_symbols(&root);
            mapping
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok(Json(Some(mapping)))
    } else {
        Ok(Json(state.code_map_mapping.clone()))
    }
}

async fn get_codemap_status(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<CodeMapStatus>, (StatusCode, String)> {
    Ok(Json(CodeMapStatus {
        warning: state.code_map_warning.clone(),
    }))
}

async fn get_codemap_file(
    State(state): State<Arc<ServeState>>,
    axum::extract::Query(params): axum::extract::Query<FileRequest>,
) -> Result<String, (StatusCode, String)> {
    let root = state.code_map_root.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "Code map mode not active".to_string(),
    ))?;

    // Prevent directory traversal
    if params.path.contains("..") {
        return Err((StatusCode::FORBIDDEN, "Invalid path".to_string()));
    }

    let full_path = root.join(&params.path);

    // Ensure the path is actually inside the root
    if !full_path.starts_with(root) {
        return Err((
            StatusCode::FORBIDDEN,
            "Path outside of codebase root".to_string(),
        ));
    }

    if full_path.exists() {
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| (StatusCode::NOT_FOUND, format!("Failed to read file: {}", e)))?;
        return Ok(content);
    }

    // Smart lookup: if the file wasn't found at the exact path, search for it by filename
    let target_name = Path::new(&params.path).file_name().and_then(|n| n.to_str());

    if let Some(name) = target_name {
        let root_clone = root.clone();
        let name_string = name.to_string();

        // Run blocking search in a separate task
        let found_path = tokio::task::spawn_blocking(move || {
            let mut matches: Vec<PathBuf> = WalkDir::new(&root_clone)
                .into_iter()
                .filter_entry(|e| {
                    let name = e.file_name().to_string_lossy();
                    !matches!(
                        name.as_ref(),
                        "node_modules" | "target" | ".git" | "dist" | "build" | ".next" | "out"
                    )
                })
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter(|e| e.file_name().to_string_lossy() == name_string)
                .map(|e| e.into_path())
                .collect();

            if matches.is_empty() {
                None
            } else if matches.len() == 1 {
                Some(matches[0].clone())
            } else {
                // Sort by depth (shallowest first), then alphabetical
                matches.sort_by(|a, b| {
                    let depth_a = a.components().count();
                    let depth_b = b.components().count();
                    if depth_a != depth_b {
                        depth_a.cmp(&depth_b)
                    } else {
                        a.cmp(b)
                    }
                });
                println!(
                    "Ambiguous file request '{}'. Found {} matches. Selecting '{}'.",
                    name_string,
                    matches.len(),
                    matches[0].display()
                );
                Some(matches[0].clone())
            }
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Some(path) = found_path {
            // Re-verify it is inside root (it should be, since we walked root)
            if !path.starts_with(root) {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Resolved path outside root".to_string(),
                ));
            }
            let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                (
                    StatusCode::NOT_FOUND,
                    format!("Failed to read resolved file: {}", e),
                )
            })?;
            return Ok(content);
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        format!("File not found: {}", params.path),
    ))
}

#[derive(Debug, Deserialize)]
struct SearchRequest {
    query: String,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    file: String,
    line: usize,
    content: String,
}

async fn get_codemap_search(
    State(state): State<Arc<ServeState>>,
    axum::extract::Query(params): axum::extract::Query<SearchRequest>,
) -> Result<Json<Vec<SearchResult>>, (StatusCode, String)> {
    let root = state.code_map_root.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "Code map mode not active".to_string(),
    ))?;

    let root_clone = root.clone();
    let query = params.query.clone();

    if query.len() < 2 {
        return Ok(Json(Vec::new()));
    }

    let results = tokio::task::spawn_blocking(move || {
        let mut matches = Vec::new();
        let walker = WalkDir::new(&root_clone).into_iter();

        for entry in walker.filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "node_modules" | "target" | ".git" | "dist" | "build" | ".next" | "out"
            )
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            // Get relative path
            let relative_path = match path.strip_prefix(&root_clone) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Skip binary files and SVG/PNG
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(
                    ext,
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "svg"
                        | "ico"
                        | "woff"
                        | "woff2"
                        | "ttf"
                        | "eot"
                ) {
                    continue;
                }
            }

            if let Ok(content) = std::fs::read_to_string(path) {
                for (i, line) in content.lines().enumerate() {
                    if line.contains(&query) {
                        matches.push(SearchResult {
                            file: relative_path.clone(),
                            line: i + 1,
                            content: line.trim().to_string(),
                        });

                        if matches.len() >= 200 {
                            return matches;
                        }
                    }
                }
            }
        }
        matches
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(results))
}
