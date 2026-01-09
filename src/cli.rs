use anyhow::{Context, Result, anyhow, bail};
use clap::{ArgAction, Parser, ValueEnum};
use dialoguer::Select;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[cfg(feature = "server")]
use oxdraw::serve::{ServeArgs, run_serve};
use oxdraw::utils::split_source_and_overrides;
use oxdraw::{Diagram, LayoutOverrides};

const DEFAULT_NEW_DIAGRAM_NAME: &str = "diagram.mmd";

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

    /// Convenience flag to force PNG output without specifying --output-format.
    #[arg(long = "png", action = ArgAction::SetTrue, conflicts_with = "output_format")]
    png: bool,

    /// Scale factor when rasterizing PNG output.
    #[arg(long = "scale", default_value_t = 10.0)]
    scale: f32,

    /// Launch the interactive editor instead of rendering once.
    #[arg(
        long = "edit",
        action = ArgAction::SetTrue,
        conflicts_with_all = ["output", "output_format"],
        requires = "input"
    )]
    edit: bool,

    /// Create a new Mermaid diagram and immediately launch the editor.
    #[arg(
        short = 'n',
        long = "new",
        action = ArgAction::SetTrue,
        conflicts_with_all = ["output", "output_format", "png", "edit"]
    )]
    new: bool,

    /// Override the host binding when using --edit or --new.
    #[arg(long = "serve-host")]
    serve_host: Option<String>,

    /// Override the port binding when using --edit or --new.
    #[arg(long = "serve-port")]
    serve_port: Option<u16>,

    /// Background color for the rendered diagram (svg only at the moment).
    #[arg(short = 'b', long = "background-color", default_value = "white")]
    background_color: String,

    /// Suppress informational output.
    #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue)]
    quiet: bool,

    /// Generate a code map from the given codebase path.
    #[arg(long = "code-map", conflicts_with = "input")]
    pub code_map: Option<String>,

    /// Generate a codedown (markdown with code mappings) from the given codebase path.
    #[arg(long = "codedown", conflicts_with = "input", conflicts_with = "code_map")]
    pub codedown: Option<String>,

    /// Augment existing markdown file with code mappings.
    #[arg(long = "augment-markdown", conflicts_with = "input", conflicts_with = "code_map", conflicts_with = "codedown")]
    pub augment_markdown: Option<String>,

    /// Repository path for augment-markdown (defaults to current directory).
    #[arg(long = "repo-path", requires = "augment_markdown")]
    pub repo_path: Option<String>,

    /// Documentation style for codedown generation.
    #[arg(long = "codedown-style", value_enum, requires = "codedown")]
    pub codedown_style: Option<CodedownStyleArg>,

    /// API Key for the LLM (optional, defaults to environment variable if not set).
    #[arg(long = "api-key")]
    pub api_key: Option<String>,

    /// Model to use for code map generation.
    #[arg(long = "model")]
    pub model: Option<String>,

    /// API URL for the LLM.
    #[arg(long = "api-url")]
    pub api_url: Option<String>,

    /// Force regeneration of the code map even if a cache exists.
    #[arg(long = "regen")]
    pub regen: bool,

    /// Custom prompt to append to the LLM instructions.
    #[arg(long = "prompt")]
    pub prompt: Option<String>,

    /// Use deterministic generation instead of AI (only for code-map).
    #[arg(long = "no-ai")]
    pub no_ai: bool,

    /// Maximum number of nodes to generate in deterministic mode.
    #[arg(long = "max-nodes", default_value_t = 20)]
    pub max_nodes: usize,

    /// Use Google Gemini API with the provided key.
    #[arg(long = "gemini", conflicts_with = "api_key")]
    pub gemini: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum OutputFormat {
    Svg,
    Png,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CodedownStyleArg {
    Architecture,
    Tutorial,
    Api,
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

    fn extension(self) -> &'static str {
        match self {
            OutputFormat::Svg => "svg",
            OutputFormat::Png => "png",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum GraphType {
    Flowchart,
}

impl GraphType {
    fn label(self) -> &'static str {
        match self {
            GraphType::Flowchart => "flowchart",
        }
    }

    fn initial_contents(self) -> &'static str {
        match self {
            GraphType::Flowchart => "graph TD\nHello World!",
        }
    }
}

fn select_graph_type() -> Result<GraphType> {
    let variants = GraphType::value_variants();

    if variants.len() == 1 {
        return Ok(variants[0]);
    }

    let options: Vec<String> = variants
        .iter()
        .map(|variant| format!("{} diagram", variant.label()))
        .collect();

    let selection = Select::new()
        .with_prompt("Select Mermaid graph type")
        .items(&options)
        .default(0)
        .interact()
        .context("graph type selection was cancelled")?;

    Ok(variants[selection])
}

fn ensure_unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }

    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "diagram".to_string());
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(String::from);

    let mut counter = 1;
    loop {
        let mut candidate = path.clone();
        let name = match &extension {
            Some(ext) => format!("{stem}{counter}.{ext}"),
            None => format!("{stem}{counter}"),
        };
        candidate.set_file_name(&name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

pub async fn run_render_or_edit(cli: RenderArgs) -> Result<()> {
    // Check for implicit codedown input via -i/--input
    if let Some(input_path) = cli.input.clone() {
        let path = PathBuf::from(&input_path);
        if path.extension().and_then(|s| s.to_str()) == Some("md") && path.exists() {
             // If the user didn't explicitly specify what to do, assume they want to view the codedown
             if cli.codedown.is_none() && cli.augment_markdown.is_none() {
                 #[cfg(feature = "server")]
                 {
                     #[cfg(not(target_arch = "wasm32"))]
                     return run_codedown(cli, input_path).await;
                     #[cfg(target_arch = "wasm32")]
                     bail!("codedown viewing is not supported in WASM");
                 }
                 #[cfg(not(feature = "server"))]
                 {
                     bail!("viewing codedown requires the 'server' feature to be enabled");
                 }
             }
        }
    }

    if let Some(code_map_path) = cli.code_map.clone() {
        #[cfg(feature = "server")]
        {
            #[cfg(not(target_arch = "wasm32"))]
            return run_code_map(cli, code_map_path).await;
            #[cfg(target_arch = "wasm32")]
            bail!("--code-map is not supported in WASM");
        }
        #[cfg(not(feature = "server"))]
        {
            bail!("--code-map requires the 'server' feature to be enabled");
        }
    } else if let Some(codedown_path) = cli.codedown.clone() {
        #[cfg(feature = "server")]
        {
            #[cfg(not(target_arch = "wasm32"))]
            return run_codedown(cli, codedown_path).await;
            #[cfg(target_arch = "wasm32")]
            bail!("--codedown is not supported in WASM");
        }
        #[cfg(not(feature = "server"))]
        {
            bail!("--codedown requires the 'server' feature to be enabled");
        }
    } else if let Some(markdown_path) = cli.augment_markdown.clone() {
        #[cfg(not(target_arch = "wasm32"))]
        return run_augment_markdown(cli, markdown_path).await;
        #[cfg(target_arch = "wasm32")]
        bail!("--augment-markdown is not supported in WASM");
    } else if cli.edit {
        #[cfg(feature = "server")]
        {
            return run_edit(cli).await;
        }
        #[cfg(not(feature = "server"))]
        {
            bail!("--edit requires the 'server' feature to be enabled");
        }
    } else if cli.new {
        #[cfg(feature = "server")]
        {
            return run_new(cli).await;
        }
        #[cfg(not(feature = "server"))]
        {
            bail!("--new requires the 'server' feature to be enabled");
        }
    }

    run_render(cli)?;
    Ok(())
}

#[cfg(not(feature = "server"))]
pub fn run_render_or_edit_sync(cli: RenderArgs) -> Result<()> {
    if cli.edit {
        bail!("--edit requires the 'server' feature to be enabled");
    } else {
        run_render(cli)?;
    }
    Ok(())
}

#[cfg(feature = "server")]
async fn run_edit(cli: RenderArgs) -> Result<()> {
    let input_source = parse_input(cli.input.as_deref())?;
    let input_path = match input_source {
        InputSource::File(path) => path,
        InputSource::Stdin => bail!("--edit requires a concrete file input"),
    };

    let canonical_input = input_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize '{}'", input_path.display()))?;

    // Try to extract code mappings from the file content
    let content = fs::read_to_string(&canonical_input)?;
    #[cfg(not(target_arch = "wasm32"))]
    let (mapping, code_map_root) = {
        // Try codemap first
        let (m, meta) = oxdraw::codemap::extract_code_mappings(&content);

        // If no codemap mappings found, try codedown
        let (m, meta) = if m.nodes.is_empty() {
            use oxdraw::codedown::extract_codedown_mappings;
            extract_codedown_mappings(&content)
        } else {
            (m, meta)
        };

        let root = if let Some(path_str) = &meta.path {
            let path = PathBuf::from(path_str);
            if path.is_absolute() && path.exists() {
                Some(path)
            } else {
                // Try to resolve relative to the input file's git root
                let input_dir = canonical_input.parent().unwrap_or(&canonical_input);
                if let Some((_, _, git_root)) = oxdraw::codemap::get_git_info(input_dir) {
                    let resolved = git_root.join(path_str);
                    if resolved.exists() {
                        Some(resolved)
                    } else {
                        // Fallback: try relative to input file directory
                        let resolved_direct = input_dir.join(path_str);
                        if resolved_direct.exists() {
                            Some(resolved_direct)
                        } else {
                            Some(path)
                        }
                    }
                } else {
                     // Fallback: try relative to input file directory
                     let resolved_direct = input_dir.join(path_str);
                     if resolved_direct.exists() {
                         Some(resolved_direct)
                     } else {
                         Some(path)
                     }
                }
            }
        } else {
            None
        };
        (Some(m), root)
    };
    #[cfg(target_arch = "wasm32")]
    let (mapping, code_map_root) = (None, None);

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
        code_map_root,
        code_map_mapping: mapping,
        code_map_warning: None,
    };

    println!("Launching editor for {}", canonical_input.display());
    println!("Loaded web UI from {}", ui_root.display());
    println!(
        "Visit http://{}:{} in your browser to begin editing",
        host, port
    );

    run_serve(serve_args, Some(ui_root)).await
}

#[cfg(feature = "server")]
async fn run_new(cli: RenderArgs) -> Result<()> {
    let RenderArgs {
        input,
        output,
        output_format,
        png,
        scale,
        serve_host,
        serve_port,
        background_color,
        quiet,
        ..
    } = cli;

    if output.is_some() {
        bail!("--new does not support specifying an output file");
    }

    if output_format.is_some() {
        bail!("--new does not support selecting an output format");
    }

    if png {
        bail!("--new does not support the --png flag");
    }

    let graph_type = select_graph_type()?;

    let mut target_path = match input {
        Some(path_str) => {
            if path_str == "-" {
                bail!("--new requires a file path, not stdin");
            }
            PathBuf::from(path_str)
        }
        None => PathBuf::from(DEFAULT_NEW_DIAGRAM_NAME),
    };

    if target_path.extension().is_none() {
        target_path.set_extension("mmd");
    }

    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory '{}'", parent.display()))?;
        }
    }

    target_path = ensure_unique_path(target_path);

    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target_path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            bail!(
                "diagram '{}' already exists; refusing to overwrite",
                target_path.display()
            );
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to create '{}'", target_path.display()));
        }
    };

    file.write_all(graph_type.initial_contents().as_bytes())?;
    file.flush()?;

    drop(file);

    let canonical_path = target_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize '{}'", target_path.display()))?;

    let display_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(DEFAULT_NEW_DIAGRAM_NAME);

    if !quiet {
        println!("Creating Mermaid graph '{display_name}'.");
        println!("Location: {}", canonical_path.display());
        println!("Graph type: {}", graph_type.label());
    }

    let edit_args = RenderArgs {
        input: Some(canonical_path.to_string_lossy().into_owned()),
        output: None,
        output_format: None,
        png: false,
        scale,
        edit: true,
        new: false,
        serve_host,
        serve_port,
        background_color,
        quiet,
        code_map: None,
        api_key: None,
        model: None,
        api_url: None,
        regen: false,
        prompt: None,
        no_ai: false,
        max_nodes: 20,
        gemini: None,
        codedown: None,
        augment_markdown: None,
        repo_path: None,
        codedown_style: None,
    };

    run_edit(edit_args).await
}

#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
async fn run_code_map(cli: RenderArgs, code_map_path: String) -> Result<()> {
    let path = PathBuf::from(&code_map_path);
    
    // Check if it's an existing .mmd file
    if path.extension().and_then(|s| s.to_str()) == Some("mmd") && path.exists() {
        let content = fs::read_to_string(&path)?;
        let (mapping, metadata) = oxdraw::codemap::extract_code_mappings(&content);
        
        let mut warning = None;
        // Check sync status
        if let Some(path_str) = &metadata.path {
             // Try to resolve the path
             let mut source_path = PathBuf::from(path_str);
             
             // If the path is relative and doesn't exist, try to resolve it relative to the git root of the input file
             if !source_path.exists() && source_path.is_relative() {
                 let input_dir = path.parent().unwrap_or(&path);
                 if let Some((_, _, git_root)) = oxdraw::codemap::get_git_info(input_dir) {
                     let resolved = git_root.join(path_str);
                     if resolved.exists() {
                         source_path = resolved;
                     }
                 }
             }

             if source_path.exists() {
                 if let Some((current_commit, current_diff, _)) = oxdraw::codemap::get_git_info(&source_path) {
                     let mut warnings = Vec::new();
                     if let Some(meta_commit) = &metadata.commit {
                         if meta_commit != &current_commit {
                             warnings.push(format!("Commit mismatch: map({}) vs HEAD({})", meta_commit, current_commit));
                         }
                     }
                     if let Some(meta_diff) = &metadata.diff_hash {
                         if meta_diff != &current_diff {
                             warnings.push("Working directory has changed since map generation".to_string());
                         }
                     }
                     if !warnings.is_empty() {
                         let msg = warnings.join("; ");
                         println!("Warning: {}", msg);
                         warning = Some(msg);
                     }
                 }
             }
        }

        let ui_root = locate_ui_dist()?;
        let host = cli.serve_host.unwrap_or_else(|| "127.0.0.1".to_string());
        let port = cli.serve_port.unwrap_or(5151);

        let serve_args = ServeArgs {
            input: path.canonicalize()?,
            host: host.clone(),
            port,
            background_color: cli.background_color,
            code_map_root: if let Some(path_str) = &metadata.path {
                // Try to resolve the path again for the server state
                let mut source_path = PathBuf::from(path_str);
                if !source_path.exists() && source_path.is_relative() {
                    let input_dir = path.parent().unwrap_or(&path);
                    if let Some((_, _, git_root)) = oxdraw::codemap::get_git_info(input_dir) {
                        let resolved = git_root.join(path_str);
                        if resolved.exists() {
                            source_path = resolved;
                        }
                    }
                }
                Some(source_path)
            } else {
                None
            },
            code_map_mapping: Some(mapping),
            code_map_warning: warning,
        };

        println!("Launching code map viewer for existing map...");
        println!("Visit http://{}:{} in your browser", host, port);

        return run_serve(serve_args, Some(ui_root)).await;
    }

    let root_path = path.canonicalize()?;
    
    let (mermaid, mapping) = oxdraw::codemap::generate_code_map(
        &root_path,
        cli.api_key,
        cli.model,
        cli.api_url,
        cli.regen,
        cli.prompt,
        cli.no_ai,
        cli.max_nodes,
        cli.gemini,
    ).await?;

    let git_info = oxdraw::codemap::get_git_info(&root_path);
    let metadata = oxdraw::codemap::CodeMapMetadata {
        path: if let Some((_, _, git_root)) = &git_info {
            // If we are in a git repo, store the path relative to the git root
            match root_path.strip_prefix(git_root) {
                Ok(p) => Some(p.to_string_lossy().to_string()),
                Err(_) => Some(root_path.to_string_lossy().to_string()),
            }
        } else {
            Some(root_path.to_string_lossy().to_string())
        },
        commit: git_info.as_ref().map(|(c, _, _)| c.clone()),
        diff_hash: git_info.as_ref().map(|(_, d, _)| *d),
    };

    let full_content = oxdraw::codemap::serialize_codemap(&mermaid, &mapping, &metadata);

    if let Some(output_path_str) = cli.output {
        if output_path_str == "-" {
             // stdout
             println!("{}", full_content);
             return Ok(());
        }

        let output_path = PathBuf::from(&output_path_str);
        let extension = output_path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

        if extension == "svg" || extension == "png" {
            // Save .mmd as well
            let mut mmd_path = output_path.clone();
            mmd_path.set_extension("mmd");
            
            fs::write(&mmd_path, &full_content)?;
            println!("Code map saved to {}", mmd_path.display());

            // Render
            let diagram = Diagram::parse(&full_content)?;
            let output_bytes = if extension == "png" {
                if cli.scale <= 0.0 {
                    bail!("--scale must be greater than zero for PNG output");
                }
                diagram.render_png(&cli.background_color, None, cli.scale)?
            } else {
                diagram.render_svg(&cli.background_color, None)?.into_bytes()
            };
            
            fs::write(&output_path, output_bytes)?;
            println!("Rendered diagram saved to {}", output_path.display());
        } else {
            fs::write(&output_path, full_content)?;
            println!("Code map saved to {}", output_path.display());
        }

        return Ok(());
    }

    // Create a temporary file for the diagram
    let temp_dir = std::env::temp_dir().join("oxdraw-codemap");
    fs::create_dir_all(&temp_dir)?;
    let diagram_path = temp_dir.join("codemap.mmd");
    fs::write(&diagram_path, full_content)?;

    let ui_root = locate_ui_dist()?;
    let host = cli.serve_host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cli.serve_port.unwrap_or(5151);

    let serve_args = ServeArgs {
        input: diagram_path,
        host: host.clone(),
        port,
        background_color: cli.background_color,
        code_map_root: if root_path.is_file() {
            root_path.parent().map(|p| p.to_path_buf())
        } else {
            Some(root_path)
        },
        code_map_mapping: Some(mapping),
        code_map_warning: None,
    };

    println!("Launching code map viewer...");
    println!("Visit http://{}:{} in your browser", host, port);

    run_serve(serve_args, Some(ui_root)).await
}

#[cfg(all(feature = "server", not(target_arch = "wasm32")))]
async fn run_codedown(cli: RenderArgs, codedown_path: String) -> Result<()> {
    use oxdraw::codedown::{CodedownStyle, generate_codedown, extract_codedown_mappings, serialize_codedown};

    let path = PathBuf::from(&codedown_path);

    // Check if it's an existing .md file
    if path.extension().and_then(|s| s.to_str()) == Some("md") && path.exists() {
        let content = fs::read_to_string(&path)?;
        let (mapping, metadata) = extract_codedown_mappings(&content);

        // Only launch viewer if mappings exist
        if !mapping.nodes.is_empty() {
            let ui_root = locate_ui_dist()?;
            let host = cli.serve_host.unwrap_or_else(|| "127.0.0.1".to_string());
            let port = cli.serve_port.unwrap_or(5151);

            let serve_args = ServeArgs {
                input: path.canonicalize()?,
                host: host.clone(),
                port,
                background_color: cli.background_color,
                code_map_root: if let Some(path_str) = &metadata.path {
                    let meta_path = PathBuf::from(path_str);
                    if meta_path.is_absolute() && meta_path.exists() {
                        Some(meta_path)
                    } else {
                        // First, try relative to current working directory (where oxdraw was called)
                        if meta_path.exists() {
                            Some(meta_path)
                        } else if path_str.is_empty() {
                            // If path is empty, assume current directory
                            Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                        } else {
                            // Fallback: Resolve relative to the markdown file location
                            let input_dir = path.parent().unwrap_or(Path::new("."));
                            let resolved_relative = input_dir.join(&meta_path);
                            if resolved_relative.exists() {
                                Some(resolved_relative)
                            } else {
                                None
                            }
                        }
                    }
                } else {
                    None
                },
                code_map_mapping: Some(mapping),
                code_map_warning: None,
            };

            println!("Launching codedown viewer for existing file...");
            println!("Visit http://{}:{} in your browser", host, port);

            return run_serve(serve_args, Some(ui_root)).await;
        }
    }

    let root_path = path.canonicalize()?;

    // Determine style
    let style = match cli.codedown_style {
        Some(CodedownStyleArg::Architecture) => CodedownStyle::Architecture,
        Some(CodedownStyleArg::Tutorial) => CodedownStyle::Tutorial,
        Some(CodedownStyleArg::Api) => CodedownStyle::Api,
        None => CodedownStyle::Architecture, // default
    };

    let (markdown, mapping) = generate_codedown(
        &root_path,
        cli.api_key,
        cli.model,
        cli.api_url,
        cli.regen,
        cli.prompt,
        style,
        cli.gemini,
    ).await?;

    let git_info = oxdraw::codemap::get_git_info(&root_path);
    let metadata = oxdraw::codemap::CodeMapMetadata {
        path: if let Some((_, _, git_root)) = &git_info {
            match root_path.strip_prefix(git_root) {
                Ok(p) => Some(p.to_string_lossy().to_string()),
                Err(_) => Some(root_path.to_string_lossy().to_string()),
            }
        } else {
            Some(root_path.to_string_lossy().to_string())
        },
        commit: git_info.as_ref().map(|(c, _, _)| c.clone()),
        diff_hash: git_info.as_ref().map(|(_, d, _)| *d),
    };

    let full_content = serialize_codedown(&markdown, &mapping, &metadata);

    if let Some(output_path_str) = cli.output {
        if output_path_str == "-" {
            println!("{}", full_content);
            return Ok(());
        }

        let output_path = PathBuf::from(&output_path_str);
        fs::write(&output_path, full_content)?;
        println!("Codedown saved to {}", output_path.display());
        return Ok(());
    }

    // Create a temporary file for the codedown
    let temp_dir = std::env::temp_dir().join("oxdraw-codedown");
    fs::create_dir_all(&temp_dir)?;
    let codedown_file_path = temp_dir.join("codedown.md");
    fs::write(&codedown_file_path, full_content)?;

    let ui_root = locate_ui_dist()?;
    let host = cli.serve_host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cli.serve_port.unwrap_or(5151);

    let serve_args = ServeArgs {
        input: codedown_file_path,
        host: host.clone(),
        port,
        background_color: cli.background_color,
        code_map_root: if root_path.is_file() {
            root_path.parent().map(|p| p.to_path_buf())
        } else {
            Some(root_path)
        },
        code_map_mapping: Some(mapping),
        code_map_warning: None,
    };

    println!("Launching codedown viewer...");
    println!("Visit http://{}:{} in your browser", host, port);

    run_serve(serve_args, Some(ui_root)).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn run_augment_markdown(cli: RenderArgs, markdown_path: String) -> Result<()> {
    use oxdraw::codedown::{augment_markdown_with_mappings, serialize_codedown};

    let markdown_file = PathBuf::from(&markdown_path);
    if !markdown_file.exists() {
        bail!("Markdown file does not exist: {}", markdown_path);
    }

    // Determine repo path
    let repo_path = if let Some(repo) = cli.repo_path {
        PathBuf::from(repo)
    } else {
        std::env::current_dir()?
    };

    if !repo_path.exists() {
        bail!("Repository path does not exist: {}", repo_path.display());
    }

    let (markdown, mapping) = augment_markdown_with_mappings(
        &markdown_file,
        &repo_path,
        cli.api_key,
        cli.model,
        cli.api_url,
        cli.gemini,
    ).await?;

    let git_info = oxdraw::codemap::get_git_info(&repo_path);
    let metadata = oxdraw::codemap::CodeMapMetadata {
        path: if let Some((_, _, git_root)) = &git_info {
            match repo_path.strip_prefix(git_root) {
                Ok(p) => Some(p.to_string_lossy().to_string()),
                Err(_) => Some(repo_path.to_string_lossy().to_string()),
            }
        } else {
            Some(repo_path.to_string_lossy().to_string())
        },
        commit: git_info.as_ref().map(|(c, _, _)| c.clone()),
        diff_hash: git_info.as_ref().map(|(_, d, _)| *d),
    };

    let full_content = serialize_codedown(&markdown, &mapping, &metadata);

    if let Some(output_path_str) = cli.output {
        if output_path_str == "-" {
            println!("{}", full_content);
            return Ok(());
        }

        let output_path = PathBuf::from(&output_path_str);
        fs::write(&output_path, full_content)?;
        println!("Augmented markdown saved to {}", output_path.display());
    } else {
        // Default: save as <original>-mapped.md
        let mut output_path = markdown_file.clone();
        let stem = output_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("document");
        output_path.set_file_name(format!("{}-mapped.md", stem));

        fs::write(&output_path, full_content)?;
        println!("Augmented markdown saved to {}", output_path.display());
    }

    Ok(())
}

fn run_render(cli: RenderArgs) -> Result<()> {
    if cli.serve_host.is_some() || cli.serve_port.is_some() {
        bail!("--serve-host/--serve-port require --edit or --new");
    }

    let input_source = parse_input(cli.input.as_deref())?;
    let format_preference = if cli.png {
        Some(OutputFormat::Png)
    } else {
        cli.output_format
    };

    let output_dest = parse_output(cli.output.as_deref(), &input_source, format_preference)?;
    let format = determine_format(format_preference, &output_dest)?;

    if format == OutputFormat::Png && cli.scale <= 0.0 {
        bail!("--scale must be greater than zero for PNG output");
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

    let output_bytes = match format {
        OutputFormat::Svg => diagram
            .render_svg(&cli.background_color, override_ref)?
            .into_bytes(),
        OutputFormat::Png => diagram.render_png(&cli.background_color, override_ref, cli.scale)?,
    };

    write_output(output_dest, &output_bytes, cli.quiet)?;

    Ok(())
}

pub async fn dispatch() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("serve") => {
            #[cfg(feature = "server")]
            {
                let serve_args = ServeArgs::parse_from(
                    std::iter::once(args[0].clone()).chain(args.iter().skip(2).cloned()),
                );
                run_serve(serve_args, None).await
            }
            #[cfg(not(feature = "server"))]
            {
                return Err(anyhow!(
                    "'serve' command requires the 'server' feature to be enabled"
                ));
            }
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

#[cfg(not(feature = "server"))]
pub fn dispatch_sync() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("render") => {
            let render_args = RenderArgs::parse_from(
                std::iter::once(args[0].clone()).chain(args.iter().skip(2).cloned()),
            );
            run_render_or_edit_sync(render_args)
        }
        _ => {
            let render_args = RenderArgs::parse_from(args);
            run_render_or_edit_sync(render_args)
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

    if let Some(bundled) = ensure_bundled_ui_dist()? {
        candidates.push(bundled);
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("frontend/out"));
    }

    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            candidates.push(PathBuf::from(ancestor).join("frontend/out"));
        }
    }

    if let Some(source) = bundled_source_dir() {
        candidates.push(source);
    }

    for candidate in candidates {
        if candidate.join("index.html").is_file() {
            return Ok(candidate);
        }
    }

    bail!(
        "unable to find built web UI assets; set OXDRAW_WEB_DIST or run 'npm install' and 'npm run build' in frontend/"
    );
}

fn bundled_source_dir() -> Option<PathBuf> {
    option_env!("OXDRAW_BUNDLED_WEB_DIST").map(PathBuf::from)
}

fn ensure_bundled_ui_dist() -> Result<Option<PathBuf>> {
    let source = match bundled_source_dir() {
        Some(path) => path,
        None => return Ok(None),
    };

    let index_file = source.join("index.html");
    if !index_file.is_file() {
        return Ok(None);
    }

    let temp_root = std::env::temp_dir()
        .join("oxdraw-ui")
        .join(env!("CARGO_PKG_VERSION"));

    let target = temp_root;

    let source_index_signature =
        std::fs::read_to_string(source.join("index.txt")).unwrap_or_default();
    let target_index_path = target.join("index.txt");

    if target_index_path.is_file() {
        if let Ok(existing) = std::fs::read_to_string(&target_index_path) {
            if existing == source_index_signature {
                return Ok(Some(target));
            }
        }
    }

    if target.exists() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("failed to clear cached UI at '{}'", target.display()))?;
    }

    copy_dir_recursive(&source, &target)?;

    Ok(Some(target))
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("failed to create directory '{}'", dest.display()))?;

    for entry in std::fs::read_dir(source)
        .with_context(|| format!("failed to list directory '{}'", source.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create directory '{}'", parent.display())
                })?;
            }
            std::fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy '{}' to '{}'",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        } else if file_type.is_symlink() {
            let resolved = std::fs::canonicalize(&src_path)
                .with_context(|| format!("failed to resolve symlink '{}'", src_path.display()))?;
            if resolved.is_dir() {
                copy_dir_recursive(&resolved, &dest_path)?;
            } else {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create directory '{}'", parent.display())
                    })?;
                }
                std::fs::copy(&resolved, &dest_path).with_context(|| {
                    format!(
                        "failed to copy '{}' to '{}'",
                        resolved.display(),
                        dest_path.display()
                    )
                })?;
            }
        }
    }

    Ok(())
}

fn parse_output(
    output: Option<&str>,
    input: &InputSource,
    format_hint: Option<OutputFormat>,
) -> Result<OutputDestination> {
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
                let ext = format_hint.unwrap_or(OutputFormat::Svg).extension();
                let default_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{name}.{ext}"))
                    .unwrap_or_else(|| format!("out.{ext}"));
                let mut default_path = path.to_path_buf();
                default_path.set_file_name(default_name);
                Ok(OutputDestination::File(default_path))
            }
            InputSource::Stdin => {
                let ext = format_hint.unwrap_or(OutputFormat::Svg).extension();
                Ok(OutputDestination::File(PathBuf::from(format!("out.{ext}"))))
            }
        },
    }
}

fn determine_format(
    preference: Option<OutputFormat>,
    output: &OutputDestination,
) -> Result<OutputFormat> {
    if let Some(fmt) = preference {
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
