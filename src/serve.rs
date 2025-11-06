use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::http::{HeaderValue, header};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceExt;
use tower::service_fn;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::diagram::decode_image_dimensions;
use crate::*;

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
}

struct ServeState {
    source_path: PathBuf,
    background: String,
    overrides: RwLock<LayoutOverrides>,
    source_lock: Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagramPayload {
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
    async fn read_diagram(&self) -> Result<(String, Diagram)> {
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
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

pub async fn run_serve(args: ServeArgs, ui_root: Option<PathBuf>) -> Result<()> {
    let initial_source = fs::read_to_string(&args.input)
        .with_context(|| format!("failed to read '{}'", args.input.display()))?;
    let (_, overrides) = split_source_and_overrides(&initial_source)?;

    let state = Arc::new(ServeState {
        source_path: args.input.clone(),
        background: args.background_color.clone(),
        overrides: RwLock::new(overrides),
        source_lock: Mutex::new(()),
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
        source_path: state.source_path.display().to_string(),
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

    if data.is_none() && mime_type.is_none() {
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

    let data_str = data
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "image payload cannot be empty".to_string(),
            )
        })?;

    let data = BASE64_STANDARD.decode(data_str.as_bytes()).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid base64 payload: {err}"),
        )
    })?;

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

#[derive(Debug, Deserialize, Default)]
struct EdgeStylePatch {
    #[serde(default)]
    line: Option<Option<EdgeKind>>,
    #[serde(default)]
    color: Option<Option<String>>,
    #[serde(default)]
    arrow: Option<Option<EdgeArrowDirection>>,
}
