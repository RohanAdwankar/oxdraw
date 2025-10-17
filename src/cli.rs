use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgAction, Parser, ValueEnum};

use crate::serve::{run_serve};
use crate::diagram::*;
use crate::serve::{split_source_and_overrides};

use crate::*;

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

#[derive(Debug, Parser)]
#[command(
    name = "oxdraw",
    about = "Render simple diagrams directly to SVG without relying on Mermaid."
)]
pub struct RenderArgs {
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

pub async fn run_render_or_edit(cli: RenderArgs) -> Result<()> {
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

pub async fn dispatch() -> Result<()> {
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

fn read_definition_and_overrides(path: &Path) -> Result<(String, LayoutOverrides)> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    split_source_and_overrides(&contents)
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
