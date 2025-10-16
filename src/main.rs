use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use clap::{ArgAction, Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceExt;
use tower::service_fn;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

mod utils;
use crate::utils::escape_xml;

const NODE_WIDTH: f32 = 140.0;
const NODE_HEIGHT: f32 = 60.0;
const NODE_SPACING: f32 = 160.0;
const START_OFFSET: f32 = 120.0;
const LAYOUT_MARGIN: f32 = 80.0;
const LAYOUT_BLOCK_START: &str = "%% OXDRAW LAYOUT START";
const LAYOUT_BLOCK_END: &str = "%% OXDRAW LAYOUT END";

#[derive(Debug, Parser)]
#[command(
    name = "oxdraw",
    about = "Render simple diagrams directly to SVG without relying on Mermaid."
)]
struct RenderArgs {
    /// Path to the input diagram file. Use '-' to read from stdin.
    #[arg(short = 'i', long = "input")]
    input: Option<String>,

    /// Path to the output file. Use '-' to write to stdout.
    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    /// Output format (defaults to the output file extension or svg).
    #[arg(short = 'e', long = "output-format")]
    output_format: Option<OutputFormat>,

    /// Launch the interactive editor instead of rendering once.
    #[arg(
        long = "edit",
        action = ArgAction::SetTrue,
        conflicts_with_all = ["output", "output_format"],
        requires = "input"
    )]
    edit: bool,

    /// Override the host binding when using --edit.
    #[arg(long = "serve-host", requires = "edit")]
    serve_host: Option<String>,

    /// Override the port binding when using --edit.
    #[arg(long = "serve-port", requires = "edit")]
    serve_port: Option<u16>,

    /// Background color for the rendered diagram (svg only at the moment).
    #[arg(short = 'b', long = "background-color", default_value = "white")]
    background_color: String,

    /// Suppress informational output.
    #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
    quiet: bool,
}

#[derive(Debug, Parser)]
#[command(name = "oxdraw serve", about = "Start the oxdraw web sync API server.")]
struct ServeArgs {
    /// Path to the diagram definition that should be served.
    #[arg(short = 'i', long = "input")]
    input: PathBuf,

    /// Address to bind the HTTP server to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on.
    #[arg(long, default_value_t = 5151)]
    port: u16,

    /// Background color for rendered SVG previews.
    #[arg(long = "background-color", default_value = "white")]
    background_color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputSource {
    Stdin,
    File(PathBuf),
}

#[derive(Debug, Clone)]
enum OutputDestination {
    Stdout,
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum OutputFormat {
    Svg,
    Png,
}

impl OutputFormat {
    fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
        {
            Some(ext) if ext == "svg" => Some(OutputFormat::Svg),
            Some(ext) if ext == "png" => Some(OutputFormat::Png),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LayoutOverrides {
    #[serde(default)]
    nodes: HashMap<String, Point>,
    #[serde(default)]
    edges: HashMap<String, EdgeOverride>,
    #[serde(default)]
    node_styles: HashMap<String, NodeStyleOverride>,
    #[serde(default)]
    edge_styles: HashMap<String, EdgeStyleOverride>,
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

async fn dispatch() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("serve") => {
            let serve_args = ServeArgs::parse_from(
                std::iter::once(args[0].clone()).chain(args.iter().skip(2).cloned()),
            );
            run_serve(serve_args, None).await
        }
        Some("render") => {
            let render_args = RenderArgs::parse_from(
                std::iter::once(args[0].clone()).chain(args.iter().skip(2).cloned()),
            );
            run_render_or_edit(render_args).await
        }
        _ => {
            let render_args = RenderArgs::parse_from(args);
            run_render_or_edit(render_args).await
        }
    }
}

async fn run_render_or_edit(cli: RenderArgs) -> Result<()> {
    if cli.edit {
        run_edit(cli).await
    } else {
        run_render(cli)
    }
}

async fn run_edit(cli: RenderArgs) -> Result<()> {
    let input_source = parse_input(cli.input.as_deref())?;
    let input_path = match input_source {
        InputSource::File(path) => path,
        InputSource::Stdin => bail!("--edit requires a concrete file input"),
    };

    let canonical_input = input_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize '{}'", input_path.display()))?;

    let ui_root = locate_ui_dist()?;

    let host = cli
        .serve_host
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cli.serve_port.unwrap_or(5151);

    let serve_args = ServeArgs {
        input: canonical_input.clone(),
        host: host.clone(),
        port,
        background_color: cli.background_color.clone(),
    };

    println!("Launching editor for {}", canonical_input.display());
    println!("Loaded web UI from {}", ui_root.display());
    println!(
        "Visit http://{}:{} in your browser to begin editing",
        host, port
    );

    run_serve(serve_args, Some(ui_root)).await
}

fn locate_ui_dist() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var("OXDRAW_WEB_DIST") {
        let custom_path = PathBuf::from(custom);
        if custom_path.join("index.html").is_file() {
            return Ok(custom_path);
        } else {
            bail!(
                "OXDRAW_WEB_DIST='{}' does not contain an index.html",
                custom_path.display()
            );
        }
    }

    let mut candidates = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("frontend/out"));
    }

    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            candidates.push(PathBuf::from(ancestor).join("frontend/out"));
        }
    }

    for candidate in candidates {
        if candidate.join("index.html").is_file() {
            return Ok(candidate);
        }
    }

    bail!(
        "unable to find built web UI assets; run 'npm install' and 'npm run build' in the frontend/ directory or set OXDRAW_WEB_DIST"
    );
}

fn run_render(cli: RenderArgs) -> Result<()> {
    let input_source = parse_input(cli.input.as_deref())?;
    let output_dest = parse_output(cli.output.as_deref(), &input_source)?;
    let format = determine_format(cli.output_format, &output_dest)?;

    if format == OutputFormat::Png {
        bail!("PNG output is not yet supported. Please target SVG for now.");
    }

    let definition_raw = load_definition(&input_source)?;
    let (definition_body, overrides) = match &input_source {
        InputSource::File(path) => read_definition_and_overrides(path)?,
        InputSource::Stdin => (definition_raw.clone(), LayoutOverrides::default()),
    };

    let diagram = Diagram::parse(&definition_body)?;
    let override_ref = if overrides.is_empty() {
        None
    } else {
        Some(&overrides)
    };

    let svg = diagram.render_svg(&cli.background_color, override_ref)?;

    write_output(output_dest, svg.as_bytes(), cli.quiet)?;

    Ok(())
}

async fn run_serve(args: ServeArgs, ui_root: Option<PathBuf>) -> Result<()> {
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

fn parse_input(input: Option<&str>) -> Result<InputSource> {
    match input {
        Some("-") => Ok(InputSource::Stdin),
        Some(path_str) => {
            let path = PathBuf::from(path_str);
            if !path.exists() {
                return Err(anyhow!("input file '{path_str}' does not exist"));
            }
            Ok(InputSource::File(path))
        }
        None => Ok(InputSource::Stdin),
    }
}

fn parse_output(output: Option<&str>, input: &InputSource) -> Result<OutputDestination> {
    match output {
        Some("-") => Ok(OutputDestination::Stdout),
        Some(path_str) => {
            let path = PathBuf::from(path_str);
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    return Err(anyhow!(
                        "output directory '{}' does not exist",
                        parent.display()
                    ));
                }
            }
            Ok(OutputDestination::File(path))
        }
        None => match input {
            InputSource::File(path) => {
                let default_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{name}.svg"))
                    .unwrap_or_else(|| "out.svg".to_string());
                let mut default_path = path.to_path_buf();
                default_path.set_file_name(default_name);
                Ok(OutputDestination::File(default_path))
            }
            InputSource::Stdin => Ok(OutputDestination::File(PathBuf::from("out.svg"))),
        },
    }
}

fn determine_format(
    explicit: Option<OutputFormat>,
    output: &OutputDestination,
) -> Result<OutputFormat> {
    if let Some(fmt) = explicit {
        return Ok(fmt);
    }

    match output {
        OutputDestination::Stdout => Ok(OutputFormat::Svg),
        OutputDestination::File(path) => OutputFormat::from_path(path).ok_or_else(|| {
            anyhow!(
                "unable to determine output format from '{}'; please specify --output-format",
                path.display()
            )
        }),
    }
}

fn load_definition(source: &InputSource) -> Result<String> {
    match source {
        InputSource::Stdin => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            if buffer.trim().is_empty() {
                Err(anyhow!("no diagram definition supplied on stdin"))
            } else {
                Ok(buffer)
            }
        }
        InputSource::File(path) => {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("failed to read '{}'", path.display()))?;
            if contents.trim().is_empty() {
                Err(anyhow!("input file '{}' was empty", path.display()))
            } else {
                Ok(contents)
            }
        }
    }
}

fn write_output(dest: OutputDestination, bytes: &[u8], quiet: bool) -> Result<()> {
    match dest {
        OutputDestination::Stdout => {
            let mut stdout = io::stdout();
            stdout.write_all(bytes)?;
            stdout.flush()?;
        }
        OutputDestination::File(path) => {
            fs::write(&path, bytes)?;
            if !quiet {
                println!("Generated diagram -> {}", path.display());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    TopDown,
    LeftRight,
    BottomTop,
    RightLeft,
}

#[derive(Debug, Clone)]
struct Diagram {
    direction: Direction,
    nodes: HashMap<String, Node>,
    order: Vec<String>,
    edges: Vec<Edge>,
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

#[derive(Debug, Clone, Serialize)]
struct DiagramPayload {
    source_path: String,
    background: String,
    auto_size: CanvasSize,
    render_size: CanvasSize,
    nodes: Vec<NodePayload>,
    edges: Vec<EdgePayload>,
    source: String,
}

#[derive(Debug, Clone, Serialize)]
struct NodePayload {
    id: String,
    label: String,
    shape: String,
    auto_position: Point,
    rendered_position: Point,
    auto_size: CanvasSize,
    rendered_size: CanvasSize,
    position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EdgePayload {
    id: String,
    from: String,
    to: String,
    label: Option<String>,
    kind: String,
    auto_points: Vec<Point>,
    rendered_points: Vec<Point>,
    points: Option<Vec<Point>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrow_direction: Option<String>,
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

struct ServeState {
    source_path: PathBuf,
    background: String,
    overrides: RwLock<LayoutOverrides>,
    source_lock: Mutex<()>,
}

impl LayoutOverrides {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.edges.is_empty()
            && self.node_styles.is_empty()
            && self.edge_styles.is_empty()
    }

    fn prune(&mut self, nodes: &HashSet<String>, edges: &HashSet<String>) {
        self.nodes.retain(|id, _| nodes.contains(id));
        self.edges.retain(|id, _| edges.contains(id));
        self.node_styles.retain(|id, _| nodes.contains(id));
        self.edge_styles.retain(|id, _| edges.contains(id));
    }
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

fn read_definition_and_overrides(path: &Path) -> Result<(String, LayoutOverrides)> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    split_source_and_overrides(&contents)
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
}

impl Diagram {
    fn parse(definition: &str) -> Result<Self> {
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

    fn render_svg(&self, background: &str, overrides: Option<&LayoutOverrides>) -> Result<String> {
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

    fn layout(&self, overrides: Option<&LayoutOverrides>) -> Result<LayoutComputation> {
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

    fn remove_node(&mut self, node_id: &str) -> bool {
        let existed = self.nodes.remove(node_id).is_some();
        if existed {
            self.order.retain(|id| id != node_id);
            self.edges
                .retain(|edge| edge.from != node_id && edge.to != node_id);
        }
        existed
    }

    fn remove_edge_by_identifier(&mut self, edge_id: &str) -> bool {
        let before = self.edges.len();
        self.edges.retain(|edge| edge_identifier(edge) != edge_id);
        before != self.edges.len()
    }

    fn to_definition(&self) -> String {
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

async fn get_diagram(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let (source, diagram) = state.read_diagram().await.map_err(internal_error)?;
    let overrides = state.current_overrides().await;

    let layout = diagram.layout(Some(&overrides)).map_err(internal_error)?;
    let geometry =
        align_geometry(&layout.final_positions, &layout.final_routes).map_err(internal_error)?;

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
        let node_size = CanvasSize {
            width: NODE_WIDTH,
            height: NODE_HEIGHT,
        };

        nodes.push(NodePayload {
            id: id.clone(),
            label: node.label.clone(),
            shape: node.shape.as_str().to_string(),
            auto_position,
            rendered_position: final_position,
            auto_size: node_size,
            rendered_size: node_size,
            position: override_position,
            fill_color,
            stroke_color,
            text_color,
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
            points: manual_points,
            color,
            arrow_direction,
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
        source,
    };

    #[cfg(debug_assertions)]
    {
        if std::env::var_os("OXDRAW_DEBUG_PAYLOAD").is_some() {
            if let Ok(json) = serde_json::to_string(&payload) {
                eprintln!("oxdraw payload: {json}");
            }
        }
    }

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
