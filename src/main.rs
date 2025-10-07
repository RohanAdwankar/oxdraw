use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use clap::{ArgAction, Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower::ServiceExt;
use tower::service_fn;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

const NODE_WIDTH: f32 = 140.0;
const NODE_HEIGHT: f32 = 60.0;
const NODE_SPACING: f32 = 160.0;
const START_OFFSET: f32 = 120.0;
const LAYOUT_MARGIN: f32 = 80.0;

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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EdgeOverride {
    #[serde(default)]
    points: Vec<Point>,
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

    let definition = load_definition(&input_source)?;
    let diagram = Diagram::parse(&definition)?;
    let overrides = match &input_source {
        InputSource::File(path) => Some(LayoutOverrides::load(&overrides_path(path))?),
        InputSource::Stdin => None,
    };

    let svg = diagram.render_svg(
        &cli.background_color,
        overrides.as_ref().filter(|o| !o.is_empty()),
    )?;

    write_output(output_dest, svg.as_bytes(), cli.quiet)?;

    Ok(())
}

async fn run_serve(args: ServeArgs, ui_root: Option<PathBuf>) -> Result<()> {
    let overrides_path = overrides_path(&args.input);
    let overrides = LayoutOverrides::load(&overrides_path)?;

    let state = Arc::new(ServeState {
        source_path: args.input.clone(),
        layout_path: overrides_path,
        background: args.background_color.clone(),
        overrides: RwLock::new(overrides),
    });

    let mut app = Router::new()
        .route("/api/diagram", get(get_diagram))
        .route("/api/diagram/svg", get(get_svg))
        .route("/api/diagram/layout", put(put_layout))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    Solid,
    Dashed,
}

#[derive(Debug, Clone, Serialize)]
struct DiagramPayload {
    source_path: String,
    background: String,
    auto_size: CanvasSize,
    render_size: CanvasSize,
    nodes: Vec<NodePayload>,
    edges: Vec<EdgePayload>,
}

#[derive(Debug, Clone, Serialize)]
struct NodePayload {
    id: String,
    label: String,
    shape: String,
    auto_position: Point,
    rendered_position: Point,
    position: Option<Point>,
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

struct ServeState {
    source_path: PathBuf,
    layout_path: PathBuf,
    background: String,
    overrides: RwLock<LayoutOverrides>,
}

impl LayoutOverrides {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("failed to read overrides '{}'", path.display()))?;
            let overrides: Self = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse overrides '{}'", path.display()))?;
            Ok(overrides)
        } else {
            Ok(Self::default())
        }
    }

    fn save(&self, path: &Path) -> Result<()> {
        if self.is_empty() {
            if path.exists() {
                fs::remove_file(path)
                    .with_context(|| format!("failed to remove overrides '{}'", path.display()))?;
            }
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            if !parent.exists() && !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create directory for overrides '{}'",
                        parent.display()
                    )
                })?;
            }
        }

        let payload = serde_json::to_string_pretty(self)?;
        fs::write(path, payload)
            .with_context(|| format!("failed to write overrides '{}'", path.display()))?;
        Ok(())
    }
}

impl ServeState {
    async fn read_diagram(&self) -> Result<Diagram> {
        let contents = tokio::fs::read_to_string(&self.source_path)
            .await
            .with_context(|| format!("failed to read '{}'", self.source_path.display()))?;
        Diagram::parse(&contents)
    }

    async fn current_overrides(&self) -> LayoutOverrides {
        self.overrides.read().await.clone()
    }

    async fn apply_update(&self, update: LayoutUpdate) -> Result<()> {
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

        overrides.save(&self.layout_path)
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
    <marker id="arrow" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto" markerUnits="strokeWidth">
      <path d="M2,2 L10,6 L2,10 z" fill="#2d3748" />
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

            let dash = if edge.kind == EdgeKind::Dashed {
                " stroke-dasharray=\"8 6\""
            } else {
                ""
            };

            if route.len() == 2 {
                let a = route[0];
                let b = route[1];
                write!(
                    svg,
                    "  <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#2d3748\" stroke-width=\"2\" marker-end=\"url(#arrow)\"{} />\n",
                    a.x, a.y, b.x, b.y, dash
                )?;
            } else {
                let points = route
                    .iter()
                    .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(
                    svg,
                    "  <polyline points=\"{}\" fill=\"none\" stroke=\"#2d3748\" stroke-width=\"2\" marker-end=\"url(#arrow)\"{} />\n",
                    points, dash
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

            match node.shape {
                NodeShape::Rectangle => write!(
                    svg,
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" ry=\"8\" fill=\"#ffffff\" stroke=\"#4a5568\" stroke-width=\"2\" />\n",
                    position.x - NODE_WIDTH / 2.0,
                    position.y - NODE_HEIGHT / 2.0,
                    NODE_WIDTH,
                    NODE_HEIGHT
                )?,
                NodeShape::Stadium => write!(
                    svg,
                    "  <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"30\" ry=\"30\" fill=\"#ffffff\" stroke=\"#4a5568\" stroke-width=\"2\" />\n",
                    position.x - NODE_WIDTH / 2.0,
                    position.y - NODE_HEIGHT / 2.0,
                    NODE_WIDTH,
                    NODE_HEIGHT
                )?,
                NodeShape::Circle => write!(
                    svg,
                    "  <ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"#ffffff\" stroke=\"#4a5568\" stroke-width=\"2\" />\n",
                    position.x,
                    position.y,
                    NODE_WIDTH / 2.0,
                    NODE_HEIGHT / 2.0
                )?,
                NodeShape::Diamond => {
                    let half_w = NODE_WIDTH / 2.0;
                    let half_h = NODE_HEIGHT / 2.0;
                    write!(
                        svg,
                        "  <polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"#ffffff\" stroke=\"#4a5568\" stroke-width=\"2\" />\n",
                        position.x,
                        position.y - half_h,
                        position.x + half_w,
                        position.y,
                        position.x,
                        position.y + half_h,
                        position.x - half_w,
                        position.y
                    )?;
                }
            }

            write!(
                svg,
                "  <text x=\"{:.1}\" y=\"{:.1}\" fill=\"#1a202c\" font-size=\"14\" text-anchor=\"middle\" dominant-baseline=\"middle\">{}</text>\n",
                position.x,
                position.y,
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
        let mut positions = HashMap::new();
        let count = self.order.len() as f32;

        let size = match self.direction {
            Direction::TopDown | Direction::BottomTop => {
                let height = START_OFFSET * 2.0 + NODE_SPACING * (count.max(1.0) - 1.0);
                let width = 480.0_f32.max(NODE_WIDTH * 2.0);
                CanvasSize {
                    width,
                    height: height.max(START_OFFSET * 2.0),
                }
            }
            Direction::LeftRight | Direction::RightLeft => {
                let width = START_OFFSET * 2.0 + NODE_SPACING * (count.max(1.0) - 1.0);
                let height = 360.0_f32.max(NODE_HEIGHT * 2.0);
                CanvasSize {
                    width: width.max(START_OFFSET * 2.0),
                    height,
                }
            }
        };

        for (index, id) in self.order.iter().enumerate() {
            let offset = START_OFFSET + index as f32 * NODE_SPACING;
            let point = match self.direction {
                Direction::TopDown => Point {
                    x: size.width / 2.0,
                    y: offset,
                },
                Direction::BottomTop => Point {
                    x: size.width / 2.0,
                    y: size.height - offset,
                },
                Direction::LeftRight => Point {
                    x: offset,
                    y: size.height / 2.0,
                },
                Direction::RightLeft => Point {
                    x: size.width - offset,
                    y: size.height / 2.0,
                },
            };
            positions.insert(id.clone(), point);
        }

        AutoLayout { positions, size }
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

fn overrides_path(input: &Path) -> PathBuf {
    let mut file_name = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
        .unwrap_or_else(|| "diagram".to_string());
    file_name.push_str(".oxdraw.json");
    input.with_file_name(file_name)
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
}

fn escape_xml(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

async fn get_diagram(
    State(state): State<Arc<ServeState>>,
) -> Result<Json<DiagramPayload>, (StatusCode, String)> {
    let diagram = state.read_diagram().await.map_err(internal_error)?;
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

        nodes.push(NodePayload {
            id: id.clone(),
            label: node.label.clone(),
            shape: node.shape.as_str().to_string(),
            auto_position,
            rendered_position: final_position,
            position: override_position,
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

        edges.push(EdgePayload {
            id: identifier,
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            kind: edge.kind.as_str().to_string(),
            auto_points,
            rendered_points: final_points,
            points: manual_points,
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
    };

    Ok(Json(payload))
}

async fn get_svg(State(state): State<Arc<ServeState>>) -> Result<Response, (StatusCode, String)> {
    let diagram = state.read_diagram().await.map_err(internal_error)?;
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

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
