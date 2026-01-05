use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{delete, get, put, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceExt;
use tower::service_fn;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use http_body_util::BodyExt;

use crate::codemap::CodeMapMapping;
use crate::database::{Database, DatabaseConfig};
use crate::*;
use crate::utils::split_source_and_overrides;
use crate::files::{DiagramFile, FileListItem};
use crate::session::{Session, create_session_cookie};
use crate::{LAYOUT_BLOCK_START, LAYOUT_BLOCK_END, NodeImage, decode_image_dimensions};
use std::collections::HashMap;

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_IMAGE_REQUEST_BYTES: usize = (MAX_IMAGE_BYTES * 4) / 3 + 1 * 1024 * 1024;

/// Arguments for running the oxdraw web server
#[derive(Debug, Clone, Parser)]
#[command(name = "oxdraw serve", about = "Start the oxdraw web sync API server.")]
pub struct ServeArgs {
    /// Path to the diagram definition that should be served (optional for multi-user mode).
    #[arg(short = 'i', long = "input")]
    pub input: Option<PathBuf>,

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
    database: Option<Database>,
    source_path: Option<PathBuf>,
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
    id: i64,
    name: String,
    filename: String,
    source_path: String,
    background: String,
    auto_size: CanvasSize,
    render_size: CanvasSize,
    nodes: Vec<NodePayload>,
    edges: Vec<EdgePayload>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    subgraphs: Vec<SubgraphPayload>,
    source: String,
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
}

#[derive(Debug, Deserialize, Default)]
struct LayoutUpdate {
    #[serde(default)]
    nodes: HashMap<String, Option<Point>>,
    #[serde(default)]
    edges: HashMap<String, Option<EdgeOverride>>,
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
    fn source_path_ref(&self) -> &PathBuf {
        self.source_path.as_ref().unwrap()
    }

    async fn read_diagram(&self) -> Result<(String, Diagram)> {
        let path = self.source_path_ref();
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let diagram = Diagram::parse(&definition)?;
        Ok((contents, diagram))
    }

    async fn current_overrides(&self) -> LayoutOverrides {
        self.overrides.read().await.clone()
    }

    async fn apply_update(&self, update: LayoutUpdate) -> Result<()> {
        let snapshot = {
            let mut overrides = self.overrides.write().await;

            for (id, value) in update.nodes {
                match value {
                    Some(point) => {
                        overrides.nodes.insert(id, point);
                    }
                    None => {
                        overrides.nodes.remove(&id);
                    }
                }
            }

            for (id, value) in update.edges {
                match value {
                    Some(edge_override) if !edge_override.points.is_empty() => {
                        overrides.edges.insert(id, edge_override);
                    }
                    _ => {
                        overrides.edges.remove(&id);
                    }
                }
            }

            overrides.clone()
        };

        self.rewrite_file_with_overrides(&snapshot).await
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
        let path = self.source_path_ref();
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let merged = merge_source_and_overrides(&definition, overrides)?;
        tokio::fs::write(path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", path.display()))?;
        Ok(())
    }

    async fn write_definition_with_overrides(
        &self,
        definition: &str,
        overrides: &LayoutOverrides,
    ) -> Result<()> {
        let merged = merge_source_and_overrides(definition, overrides)?;
        let _guard = self.source_lock.lock().await;
        let path = self.source_path_ref();
        tokio::fs::write(path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", path.display()))?;
        Ok(())
    }

    async fn remove_node(&self, node_id: &str) -> Result<bool> {
        let diagram = {
            let _guard = self.source_lock.lock().await;
            let path = self.source_path_ref();
            let source = tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read '{}'", path.display()))?;
            let mut diagram = Diagram::parse(&source)?;
            if diagram.nodes.len() == 1 && diagram.nodes.contains_key(node_id) {
                bail!("diagram must contain at least one node");
            }
            if !diagram.remove_node(node_id) {
                return Ok(false);
            }
            let rewritten = diagram.to_definition();
            let path = self.source_path_ref();
            tokio::fs::write(path, rewritten.as_bytes())
                .await
                .with_context(|| format!("failed to write '{}'", path.display()))?;
            diagram
        };

        self.prune_overrides_for(&diagram).await?;
        Ok(true)
    }

    async fn remove_edge(&self, edge_id: &str) -> Result<bool> {
        let diagram = {
            let _guard = self.source_lock.lock().await;
            let path = self.source_path_ref();
            let source = tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read '{}'", path.display()))?;
            let mut diagram = Diagram::parse(&source)?;
            if !diagram.remove_edge_by_identifier(edge_id) {
                return Ok(false);
            }
            let rewritten = diagram.to_definition();
            tokio::fs::write(path, rewritten.as_bytes())
                .await
                .with_context(|| format!("failed to write '{}'", path.display()))?;
            diagram
        };

        self.prune_overrides_for(&diagram).await?;
        Ok(true)
    }

    async fn set_node_image(&self, node_id: &str, image: Option<NodeImage>) -> Result<()> {
        let overrides_snapshot = self.overrides.read().await.clone();
        let _guard = self.source_lock.lock().await;
        let path = self.source_path_ref();
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read '{}'", path.display()))?;
        let (definition, _) = split_source_and_overrides(&contents)?;
        let mut diagram = Diagram::parse(&definition)?;
        let Some(node) = diagram.nodes.get_mut(node_id) else {
            bail!("node '{node_id}' not found");
        };
        node.image = image;
        let rewritten = diagram.to_definition();
        let merged = merge_source_and_overrides(&rewritten, &overrides_snapshot)?;
        let path = self.source_path_ref();
        tokio::fs::write(path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", path.display()))?;
        Ok(())
    }

    async fn update_node_image_padding(&self, node_id: &str, padding: f32) -> Result<()> {
        let overrides_snapshot = self.overrides.read().await.clone();
        let _guard = self.source_lock.lock().await;
        let path = self.source_path_ref();
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read '{}'", path.display()))?;
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
        tokio::fs::write(path, merged.as_bytes())
            .await
            .with_context(|| format!("failed to write '{}'", path.display()))?;
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
    let database = if args.input.is_none() {
        Some(Database::new(DatabaseConfig::default()).await?)
    } else {
        None
    };

    let initial_source = if let Some(ref input_path) = args.input {
        let content = fs::read_to_string(input_path)
            .with_context(|| format!("failed to read '{}'", input_path.display()))?;
        let (_, overrides) = split_source_and_overrides(&content)?;
        Some(content)
    } else {
        None
    };

    let state = Arc::new(ServeState {
        database,
        source_path: args.input.clone(),
        background: args.background_color.clone(),
        overrides: RwLock::new(if let Some(ref source) = initial_source {
            let (_, overrides) = split_source_and_overrides(source).unwrap_or_default();
            overrides
        } else {
            LayoutOverrides::default()
        }),
        source_lock: Mutex::new(()),
        code_map_root: args.code_map_root,
        code_map_mapping: args.code_map_mapping,
        code_map_warning: args.code_map_warning,
    });

    let mut app = Router::new()
        .route("/api/sessions/current", get(get_current_session))
        .route("/api/diagrams", get(list_diagrams).post(create_diagram))
        .route("/api/diagrams/:id", get(get_diagram_by_id).delete(delete_diagram))
        .route("/api/diagrams/:id/content", put(update_diagram_content))
        .route("/api/diagrams/:id/name", put(update_diagram_name))
        .route("/api/diagrams/:id/duplicate", post(duplicate_diagram))
        .route("/api/diagrams/:id/svg", get(get_diagram_svg))
        .route("/api/diagrams/:id/layout", put(put_layout_db))
        .route("/api/diagrams/:id/style", put(put_style_db))
        .route("/api/diagrams/:id/source", get(get_diagram_source).put(put_diagram_source))
        .route("/api/diagrams/:id/nodes/:node_id/image", put(put_node_image_db))
        .route("/api/diagrams/:id/nodes/:node_id", delete(delete_node_db))
        .route("/api/diagrams/:id/edges/:edge_id", delete(delete_edge_db))
        .route("/api/codemap/mapping", get(get_codemap_mapping))
        .route("/api/codemap/status", get(get_codemap_status))
        .route("/api/codemap/file", get(get_codemap_file))
        .route("/api/codemap/open", axum::routing::post(open_in_editor))
        .layer(DefaultBodyLimit::max(MAX_IMAGE_REQUEST_BYTES));

    if args.input.is_some() {
        app = app
            .route("/api/diagram", get(get_diagram))
            .route("/api/diagram/svg", get(get_svg))
            .route("/api/diagram/layout", put(put_layout))
            .route("/api/diagram/style", put(put_style))
            .route("/api/diagram/source", get(get_source).put(put_source))
            .route("/api/diagram/nodes/:id/image", put(put_node_image))
            .route("/api/diagram/nodes/:id", delete(delete_node))
            .route("/api/diagram/edges/:id", delete(delete_edge));
    }

    if let Some(root) = ui_root {
        let static_dir = ServeDir::new(root.clone())
            .append_index_html_on_directories(true)
            .fallback(ServeFile::new(root.join("index.html")));
        let dir_for_service = static_dir.clone();

        let static_service = service_fn(move |req| {
            let svc = dir_for_service.clone();
            async move {
                match svc.oneshot(req).await {
                    Ok(response) => {
                        let (parts, body) = response.into_parts();
                        let boxed = body.boxed_unsync();
                        Ok(Response::from_parts(parts, axum::body::Body::new(boxed)))
                    }
                    Err(error) => {
                        let message = format!("Static file error: {error}");
                        Ok((StatusCode::INTERNAL_SERVER_ERROR, message).into_response())
                    }
                }
            }
        });

        app = app.fallback_service(static_service);
    }

    let app = app
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let addr = format!("{}:{}", args.host, args.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind HTTP server to {addr}"))?;

    let mode = if args.input.is_some() {
        format!("file mode ({})", args.input.as_ref().unwrap().display())
    } else {
        "multi-user database mode".to_string()
    };
    println!("oxdraw server listening on http://{addr} ({mode})");
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

    let payload = DiagramPayload {
        id: 0,
        name: String::new(),
        filename: state.source_path.as_ref().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default(),
        source_path: state.source_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        background: state.background.clone(),
        auto_size: layout.auto_size,
        render_size: CanvasSize {
            width: geometry.width,
            height: geometry.height,
        },
        nodes,
        edges,
        subgraphs,
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

async fn put_layout_db(
    State(state): State<Arc<ServeState>>,
    AxumPath(diagram_id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
    Json(update): Json<LayoutUpdate>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, diagram_id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, mut overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    for (id, value) in update.nodes {
        match value {
            Some(point) => {
                overrides.nodes.insert(id, point);
            }
            None => {
                overrides.nodes.remove(&id);
            }
        }
    }

    for (id, value) in update.edges {
        match value {
            Some(edge_override) if !edge_override.points.is_empty() => {
                overrides.edges.insert(id, edge_override);
            }
            _ => {
                overrides.edges.remove(&id);
            }
        }
    }

    let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();
    overrides.prune(&node_ids, &edge_ids);

    let new_definition = diagram.to_definition();
    let merged = merge_source_and_overrides(&new_definition, &overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &merged).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn put_style_db(
    State(state): State<Arc<ServeState>>,
    AxumPath(diagram_id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
    Json(update): Json<StyleUpdate>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, diagram_id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, mut overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

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

    let diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();
    overrides.prune(&node_ids, &edge_ids);

    let new_definition = diagram.to_definition();
    let merged = merge_source_and_overrides(&new_definition, &overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &merged).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

    let content = tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Failed to read file: {}", e)))?;

    Ok(content)
}

fn extract_session_id(headers: &axum::http::HeaderMap) -> Option<String> {
    headers.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|cookie| {
            cookie.trim().split_once('=')
                .filter(|(name, _)| *name == "oxdraw_session")
                .map(|(_, value)| value.to_string())
        })
}

async fn get_current_session(
    State(state): State<Arc<ServeState>>,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, impl IntoResponse), (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = if let Some(sid) = extract_session_id(&headers) {
        sid
    } else {
        let session = Session::create(pool).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        session.id
    };

    let session = Session::get_by_id(pool, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let session = match session {
        Some(s) => {
            s.touch(pool).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            s
        }
        None => {
            Session::create(pool).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        }
    };

    let diagrams = DiagramFile::list_by_session(pool, &session.id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let current_diagram_id = if diagrams.is_empty() {
        let new_diagram = DiagramFile::create(pool, &session.id, "New Diagram", None).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Some(new_diagram.id)
    } else {
        diagrams.first().map(|d| d.id)
    };

    let cookie = create_session_cookie(&session.id);
    let json_value = json!({
        "sessionId": session.id,
        "diagrams": diagrams,
        "currentDiagramId": current_diagram_id,
    });

    let mut response = Json(json_value).into_response();
    response.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?);

    Ok((StatusCode::OK, response))
}

async fn list_diagrams(
    State(state): State<Arc<ServeState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<FileListItem>>, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let session = Session::get_by_id(pool, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if session.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Invalid session".to_string()));
    }

    let diagrams = DiagramFile::list_by_session(pool, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(diagrams))
}

#[derive(Debug, Deserialize)]
struct CreateDiagramRequest {
    name: Option<String>,
    template: Option<String>,
}

async fn create_diagram(
    State(state): State<Arc<ServeState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateDiagramRequest>,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let session = Session::get_by_id(pool, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if session.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Invalid session".to_string()));
    }

    let name = req.name.unwrap_or_else(|| "New Diagram".to_string());
    let template = req.template.as_deref();

    let diagram_file = DiagramFile::create(pool, &session_id, &name, template).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    session.unwrap().touch(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    convert_diagram_to_payload(pool, &state, diagram_file).await
}

async fn get_diagram_by_id(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let session = Session::get_by_id(pool, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if session.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "Invalid session".to_string()));
    }

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    session.unwrap().touch(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    convert_diagram_to_payload(pool, &state, diagram_file).await
}

async fn convert_diagram_to_payload(
    _pool: &sqlx::SqlitePool,
    state: &Arc<ServeState>,
    diagram_file: DiagramFile,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let (definition, overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let layout = diagram.layout(Some(&overrides))
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let geometry = align_geometry(
        &layout.final_positions,
        &layout.final_routes,
        &diagram.edges,
        &diagram.subgraphs,
        &diagram.nodes,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut nodes = Vec::new();
    for id in &diagram.order {
        let node = diagram.nodes.get(id)
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, format!("node '{id}' missing from diagram")))?;
        let auto_position = layout.auto_positions.get(id)
            .copied()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, format!("auto layout missing node '{id}'")))?;
        let final_position = layout.final_positions.get(id)
            .copied()
            .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, format!("final layout missing node '{id}'")))?;
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
        let auto_points = layout.auto_routes.get(&identifier)
            .cloned()
            .unwrap_or_default();
        let final_points = layout.final_routes.get(&identifier)
            .cloned()
            .unwrap_or_default();
        let manual_points = overrides.edges.get(&identifier)
            .map(|edge_override| edge_override.points.clone());
        let style = overrides.edge_styles.get(&identifier);
        let line_kind = style.and_then(|s| s.line)
            .unwrap_or(edge.kind)
            .as_str()
            .to_string();
        let color = style.and_then(|s| s.color.clone());
        let arrow_direction = style.and_then(|s| s.arrow)
            .map(|direction| direction.as_str().to_string());

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

    let payload = DiagramPayload {
        id: diagram_file.id,
        name: diagram_file.name.clone(),
        filename: diagram_file.filename.clone(),
        source_path: diagram_file.filename.clone(),
        background: state.background.clone(),
        auto_size: layout.auto_size,
        render_size: CanvasSize {
            width: geometry.width,
            height: geometry.height,
        },
        nodes,
        edges,
        subgraphs,
        source: diagram_file.content.clone(),
    };

    Ok(Json(payload))
}

async fn get_diagram_svg(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let override_ref = if overrides.is_empty() { None } else { Some(&overrides) };
    let svg = diagram.render_svg(&state.background, override_ref)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut response = Response::new(svg.into());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"),
    );
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct UpdateDiagramContentRequest {
    content: String,
}

async fn update_diagram_content(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateDiagramContentRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    diagram_file.update_content(pool, &req.content).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct UpdateDiagramNameRequest {
    name: String,
}

async fn update_diagram_name(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateDiagramNameRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let now = chrono::Utc::now();
    let filename = if req.name.ends_with(".mmd") {
        req.name.clone()
    } else {
        format!("{}.mmd", req.name)
    };

    sqlx::query("UPDATE diagrams SET name = ?, filename = ?, updated_at = ? WHERE id = ?")
        .bind(&req.name)
        .bind(&filename)
        .bind(now.to_rfc3339())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn duplicate_diagram(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let new_diagram = diagram_file.duplicate(pool, None).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    convert_diagram_to_payload(pool, &state, new_diagram).await
}

async fn delete_diagram(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    diagram_file.delete(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_diagram_source(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Json<SourcePayload>, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, _) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(SourcePayload { source: definition }))
}

async fn put_diagram_source(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<i64>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SourceUpdateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (_, mut existing_overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let new_diagram = Diagram::parse(&req.source)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid diagram source: {e}")))?;

    let node_ids: HashSet<String> = new_diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = new_diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();
    existing_overrides.prune(&node_ids, &edge_ids);

    let content = merge_source_and_overrides(&req.source, &existing_overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &content).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn delete_node_db(
    State(state): State<Arc<ServeState>>,
    AxumPath((diagram_id, node_id)): AxumPath<(i64, String)>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, diagram_id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    if diagram.nodes.len() == 1 && diagram.nodes.contains_key(&node_id) {
        return Err((StatusCode::BAD_REQUEST, "diagram must contain at least one node".to_string()));
    }

    if !diagram.remove_node(&node_id) {
        return Err((StatusCode::NOT_FOUND, format!("node '{node_id}' not found")));
    }

    let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();

    let mut updated_overrides = overrides;
    updated_overrides.prune(&node_ids, &edge_ids);

    let new_definition = diagram.to_definition();
    let merged = merge_source_and_overrides(&new_definition, &updated_overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &merged).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn delete_edge_db(
    State(state): State<Arc<ServeState>>,
    AxumPath((diagram_id, edge_id)): AxumPath<(i64, String)>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, diagram_id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    if !diagram.remove_edge_by_identifier(&edge_id) {
        return Err((StatusCode::NOT_FOUND, format!("edge '{edge_id}' not found")));
    }

    let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();

    let mut updated_overrides = overrides;
    updated_overrides.prune(&node_ids, &edge_ids);

    let new_definition = diagram.to_definition();
    let merged = merge_source_and_overrides(&new_definition, &updated_overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &merged).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn put_node_image_db(
    State(state): State<Arc<ServeState>>,
    AxumPath((diagram_id, node_id)): AxumPath<(i64, String)>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<NodeImageUpdateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let pool = state.database.as_ref()
        .ok_or((StatusCode::NOT_FOUND, "Database mode not active".to_string()))?
        .pool();

    let session_id = extract_session_id(&headers)
        .ok_or((StatusCode::UNAUTHORIZED, "No session".to_string()))?;

    let diagram_file = DiagramFile::get_by_id(pool, diagram_id, &session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let diagram_file = diagram_file
        .ok_or((StatusCode::NOT_FOUND, "Diagram not found".to_string()))?;

    let (definition, mut overrides) = split_source_and_overrides(&diagram_file.content)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let mut diagram = Diagram::parse(&definition)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let node = diagram.nodes.get_mut(&node_id)
        .ok_or((StatusCode::NOT_FOUND, format!("node '{node_id}' not found")))?;

    let sanitized_padding = payload.padding.map(|value| {
        if value.is_nan() || !value.is_finite() || value < 0.0 { 0.0 } else { value }
    });

    if payload.data.is_none() || payload.data.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
        node.image = None;
    } else {
        let mime_type = payload.mime_type
            .as_ref()
            .map(|s| s.trim())
            .filter(|v| !v.is_empty())
            .ok_or((StatusCode::BAD_REQUEST, "mime_type is required".to_string()))?
            .to_string();

        let data_str = payload.data.as_ref().unwrap().trim();
        let data = BASE64_STANDARD.decode(data_str.as_bytes())
            .map_err(|err| (StatusCode::BAD_REQUEST, format!("invalid base64 payload: {err}")))?;

        if data.len() > MAX_IMAGE_BYTES {
            return Err((StatusCode::BAD_REQUEST, format!("image too large (max {} bytes)", MAX_IMAGE_BYTES)));
        }

        let (width, height) = decode_image_dimensions(&mime_type, &data)
            .map_err(|err| (StatusCode::BAD_REQUEST, format!("unsupported image: {err}")))?;

        node.image = Some(NodeImage {
            mime_type,
            data,
            width,
            height,
            padding: sanitized_padding.unwrap_or(0.0),
        });
    }

    let node_ids: HashSet<String> = diagram.nodes.keys().cloned().collect();
    let edge_ids: HashSet<String> = diagram.edges.iter()
        .map(|e| edge_identifier(e))
        .collect();

    overrides.prune(&node_ids, &edge_ids);

    let new_definition = diagram.to_definition();
    let merged = merge_source_and_overrides(&new_definition, &overrides)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    diagram_file.update_content(pool, &merged).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
