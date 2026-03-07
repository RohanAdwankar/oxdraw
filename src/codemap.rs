use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::Diagram;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeMapMapping {
    pub nodes: HashMap<String, CodeLocation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeLocation {
    pub file: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub symbol: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LlmResponse {
    mermaid: String,
    mapping: HashMap<String, CodeLocation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    commit: String,
    diff_hash: u64,
    mermaid: String,
    mapping: CodeMapMapping,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeMapMetadata {
    pub path: Option<String>,
    pub commit: Option<String>,
    pub diff_hash: Option<u64>,
}

pub async fn generate_code_map(
    path: &Path,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
    regen: bool,
    custom_prompt: Option<String>,
    no_ai: bool,
    uml: bool,
    max_nodes: usize,
    gemini_key: Option<String>,
) -> Result<(String, CodeMapMapping)> {
    let deterministic_mode = no_ai || uml;
    let git_info = get_git_info(path);

    let project_dirs = ProjectDirs::from("", "", "oxdraw")
        .ok_or_else(|| anyhow!("Could not determine config directory"))?;
    let config_dir = project_dirs.config_dir();
    fs::create_dir_all(config_dir).context("Failed to create config directory")?;

    let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    abs_path.hash(&mut hasher);
    uml.hash(&mut hasher);
    let path_hash = hasher.finish();
    let cache_path = config_dir.join(format!("cache_{:x}.json", path_hash));

    if !regen {
        if let Some((commit, diff_hash, _)) = &git_info {
            if let Ok(cache_content) = fs::read_to_string(&cache_path) {
                if let Ok(cache) = serde_json::from_str::<CacheEntry>(&cache_content) {
                    if cache.commit == *commit && cache.diff_hash == *diff_hash {
                        println!(
                            "Using cached code map for commit {} (diff hash: {:x})",
                            commit, diff_hash
                        );
                        return Ok((cache.mermaid, cache.mapping));
                    }
                }
            }
        }
    }

    if deterministic_mode {
        println!("Generating deterministic code map (no AI)...");
        let (mermaid, mapping) = if uml {
            generate_deterministic_uml_map(path, max_nodes)?
        } else {
            generate_deterministic_map(path, max_nodes)?
        };

        // Cache the result
        if let Some((commit, diff_hash, _)) = git_info {
            let cache_entry = CacheEntry {
                commit,
                diff_hash,
                mermaid: mermaid.clone(),
                mapping: CodeMapMapping {
                    nodes: mapping.nodes.clone(),
                },
            };
            if let Ok(json) = serde_json::to_string_pretty(&cache_entry) {
                let _ = fs::write(cache_path, json);
            }
        }
        return Ok((mermaid, mapping));
    }

    println!("Scanning codebase at {}...", path.display());
    let (file_summaries, granularity) = scan_codebase(path)?;

    println!(
        "Found {} files. Generating code map...",
        file_summaries.len()
    );

    let base_prompt = if uml {
        match granularity {
            Granularity::File => "You are an expert software engineer. Analyze the following source file and generate a Mermaid classDiagram that captures key classes/types, their relationships, and main members.

        Use Mermaid classDiagram relationship syntax (inheritance, realization, composition, aggregation, association) when supported by the code.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",

            Granularity::Directory => "You are an expert software architect. Analyze the files in the following directory and generate a Mermaid classDiagram that captures important classes/types and their relationships.

        Use Mermaid classDiagram relationship syntax (inheritance, realization, composition, aggregation, association) when supported by the code.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",

            Granularity::Repo => "You are an expert software architect. Analyze the following codebase and generate a Mermaid classDiagram that captures high-level domain types and their relationships.

        Use Mermaid classDiagram relationship syntax (inheritance, realization, composition, aggregation, association) when supported by the code.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",
        }
    } else {
        match granularity {
            Granularity::File => "You are an expert software engineer. Analyze the following source file and generate a Mermaid flowchart that explains its internal logic, control flow, and structure.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",

            Granularity::Directory => "You are an expert software architect. Analyze the files in the following directory and generate a Mermaid flowchart that explains the relationships and data flow between them.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",

            Granularity::Repo => "You are an expert software architect. Analyze the following codebase and generate a Mermaid flowchart that explains the high-level architecture and data flow.

        For each node in the diagram, you MUST provide a mapping to the specific code location that the node represents.
        Prefer using symbol names (functions, classes, structs, etc.) over line numbers when possible, as line numbers are brittle.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.",
        }
    };

    let mermaid_example = if uml {
        "classDiagram\\n    class A\\n    class B\\n    A <|-- B"
    } else {
        "graph TD\\n    A[Node Label] --> B[Another Node]"
    };

    let mut prompt = format!(
        "{}

        Return ONLY a JSON object with the following structure. Do not include other components of mermaid syntax such as as style This is the JSON schema to follow:
        {{
            \"mermaid\": \"{}\",
            \"mapping\": {{
                \"A\": {{ \"file\": \"src/main.rs\", \"symbol\": \"main\", \"start_line\": 10, \"end_line\": 20 }},
                \"B\": {{ \"file\": \"src/lib.rs\", \"symbol\": \"MyStruct\", \"start_line\": 5, \"end_line\": 15 }}
            }}
        }}
        ", base_prompt, mermaid_example
    );

    if let Some(custom) = custom_prompt {
        prompt.push_str(&format!("\n\nUser Instructions:\n{}\n", custom));
    }

    prompt.push_str(&format!(
        "\n\nHere are the files:\n\n{}",
        file_summaries.join("\n\n")
    ));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let (url, model) = if let Some(key) = &gemini_key {
        let model = model.unwrap_or_else(|| "gemini-2.0-flash".to_string());
        (
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, key
            ),
            model,
        )
    } else {
        (
            api_url.unwrap_or_else(|| "http://localhost:8080/v1/responses".to_string()),
            model.unwrap_or_else(|| "gemini-2.0-flash".to_string()),
        )
    };

    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 4;

    loop {
        attempts += 1;
        if attempts > MAX_ATTEMPTS {
            bail!(
                "Failed to generate valid code map after {} attempts",
                MAX_ATTEMPTS
            );
        }

        if attempts > 1 {
            println!("Attempt {}/{}...", attempts, MAX_ATTEMPTS);
        }

        let mut request = client.post(&url);

        if gemini_key.is_some() {
            let body = serde_json::json!({
                "contents": [{
                    "parts": [{
                        "text": prompt
                    }]
                }]
            });
            request = request.json(&body);
        } else {
            let mut body = HashMap::new();
            body.insert("model", model.clone());
            body.insert("input", prompt.clone());
            request = request.json(&body);

            if let Some(key) = &api_key {
                request = request.header("Authorization", format!("Bearer {}", key));
            }
        }

        let response = request
            .send()
            .await
            .context("Failed to send request to LLM")?;

        if !response.status().is_success() {
            let text = response.text().await?;
            return Err(anyhow!("LLM API returned error: {}", text));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse LLM response JSON")?;

        // Try to extract text from different possible formats
        let output_text = if let Some(text) =
            response_json.get("output_text").and_then(|v| v.as_str())
        {
            text.to_string()
        } else if let Some(candidates) = response_json.get("candidates").and_then(|v| v.as_array())
        {
            // Gemini format
            candidates
                .first()
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
                .and_then(|p| p.first())
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow!("Could not find content in Gemini response"))?
                .to_string()
        } else if let Some(choices) = response_json.get("choices").and_then(|v| v.as_array()) {
            // Standard OpenAI format
            choices
                .first()
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .ok_or_else(|| anyhow!("Could not find content in OpenAI response"))?
                .to_string()
        } else {
            // Fallback for the custom format
            if let Some(output) = response_json.get("output").and_then(|v| v.as_array()) {
                if let Some(first) = output.first() {
                    if let Some(content) = first.get("content").and_then(|v| v.as_array()) {
                        if let Some(first_content) = content.first() {
                            if let Some(text) = first_content.get("text").and_then(|v| v.as_str()) {
                                text.to_string()
                            } else {
                                return Err(anyhow!("Unknown response format (deep nested)"));
                            }
                        } else {
                            return Err(anyhow!("Unknown response format (empty content)"));
                        }
                    } else {
                        return Err(anyhow!("Unknown response format (no content array)"));
                    }
                } else {
                    return Err(anyhow!("Unknown response format (empty output)"));
                }
            } else {
                return Err(anyhow!("Unknown response format: {:?}", response_json));
            }
        };

        // Clean up the output text (remove markdown code blocks if present)
        let clean_json = output_text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let result: LlmResponse = match serde_json::from_str(clean_json) {
            Ok(r) => r,
            Err(e) => {
                println!("Failed to parse JSON: {}", e);
                prompt.push_str(&format!("\n\nYour previous response was not valid JSON: {}. Please return ONLY valid JSON.", e));
                continue;
            }
        };

        // Validate the result
        match validate_response(&result) {
            Ok(_) => {
                // Save to cache if we have git info
                if let Some((commit, diff_hash, _)) = git_info {
                    let cache_entry = CacheEntry {
                        commit,
                        diff_hash,
                        mermaid: result.mermaid.clone(),
                        mapping: CodeMapMapping {
                            nodes: result.mapping.clone(),
                        },
                    };
                    if let Ok(json) = serde_json::to_string_pretty(&cache_entry) {
                        let _ = fs::write(cache_path, json);
                    }
                }
                return Ok((
                    result.mermaid,
                    CodeMapMapping {
                        nodes: result.mapping,
                    },
                ));
            }
            Err(e) => {
                println!("Validation failed: {}", e);
                prompt.push_str(&format!("\n\nYour previous response failed validation: {}. Please fix the diagram and mapping.", e));
                continue;
            }
        }
    }
}

fn validate_response(response: &LlmResponse) -> Result<()> {
    // 1. Parse Mermaid
    let diagram =
        Diagram::parse(&response.mermaid).context("Failed to parse generated Mermaid diagram")?;

    // 2. Check Mapping Completeness
    for node_id in diagram.nodes.keys() {
        if !response.mapping.contains_key(node_id) {
            bail!(
                "Node '{}' is present in the diagram but missing from the mapping object.",
                node_id
            );
        }
    }

    // 3. Check for Isolated Nodes (if more than 1 node)
    if diagram.nodes.len() > 1 {
        let mut connected_nodes = HashSet::new();
        for edge in &diagram.edges {
            connected_nodes.insert(&edge.from);
            connected_nodes.insert(&edge.to);
        }

        for node_id in diagram.nodes.keys() {
            if !connected_nodes.contains(node_id) {
                bail!(
                    "Node '{}' is isolated (not connected to any other node). All nodes must be connected.",
                    node_id
                );
            }
        }
    }

    Ok(())
}

pub fn get_git_info(path: &Path) -> Option<(String, u64, PathBuf)> {
    // Get git root
    let root_output = Command::new("git")
        .args(&["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()?;

    if !root_output.status.success() {
        return None;
    }
    let root_str = String::from_utf8_lossy(&root_output.stdout)
        .trim()
        .to_string();
    let root_path = PathBuf::from(root_str);

    // Get commit hash
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Get diff hash
    let diff_output = Command::new("git")
        .args(&["diff", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;

    let mut hasher = DefaultHasher::new();
    diff_output.stdout.hash(&mut hasher);
    let diff_hash = hasher.finish();

    Some((commit, diff_hash, root_path))
}

#[derive(Debug, PartialEq)]
enum Granularity {
    Repo,
    Directory,
    File,
}

fn scan_codebase(root_path: &Path) -> Result<(Vec<String>, Granularity)> {
    let mut summaries = Vec::new();
    let mut total_chars = 0;
    const MAX_TOTAL_CHARS: usize = 100_000; // Limit total context size

    if root_path.is_file() {
        if let Ok(content) = fs::read_to_string(root_path) {
            let file_name = root_path.file_name().unwrap_or_default().to_string_lossy();
            summaries.push(format!("File: {}\n```\n{}\n```", file_name, content));
            return Ok((summaries, Granularity::File));
        }
    }

    // Basic ignore list
    let include_exts = vec![
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cpp", "h",
    ];
    let ignore_dirs = vec![
        "target",
        "node_modules",
        ".git",
        "dist",
        "build",
        ".next",
        "out",
    ];

    let walker = WalkDir::new(root_path).into_iter();

    for entry in walker.filter_entry(|e| {
        let file_name = e.file_name().to_string_lossy();
        !ignore_dirs.iter().any(|d| file_name == *d)
    }) {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if include_exts.contains(&ext) {
                if let Ok(content) = fs::read_to_string(path) {
                    // Truncate if too large
                    let truncated = if content.len() > 10000 {
                        format!("{}... (truncated)", &content[..10000])
                    } else {
                        content
                    };

                    if total_chars + truncated.len() > MAX_TOTAL_CHARS {
                        break; // Stop if we exceed the budget
                    }

                    total_chars += truncated.len();

                    // Get relative path
                    let rel_path = path
                        .strip_prefix(root_path)
                        .unwrap_or(path)
                        .to_string_lossy();
                    summaries.push(format!("File: {}\n```\n{}\n```", rel_path, truncated));
                }
            }
        }
    }

    // Determine if it's a repo or just a directory
    let granularity = if root_path.join(".git").exists() {
        Granularity::Repo
    } else {
        Granularity::Directory
    };

    Ok((summaries, granularity))
}

pub fn extract_code_mappings(source: &str) -> (CodeMapMapping, CodeMapMetadata) {
    let mut nodes = HashMap::new();
    let mut metadata = CodeMapMetadata {
        path: None,
        commit: None,
        diff_hash: None,
    };

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("%% OXDRAW CODE") {
            // Parse: %% OXDRAW CODE <NodeID> <FilePath> [line:<Start>-<End>] [def:<Symbol>]
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 4 {
                let node_id = parts[3].to_string();
                let file_path = parts[4].to_string();
                let mut start_line = None;
                let mut end_line = None;
                let mut symbol = None;

                for part in parts.iter().skip(5) {
                    if let Some(range) = part.strip_prefix("line:") {
                        if let Some((start, end)) = range.split_once('-') {
                            start_line = start.parse().ok();
                            end_line = end.parse().ok();
                        }
                    } else if let Some(sym) = part.strip_prefix("def:") {
                        symbol = Some(sym.to_string());
                    }
                }

                nodes.insert(
                    node_id,
                    CodeLocation {
                        file: file_path,
                        start_line,
                        end_line,
                        symbol,
                    },
                );
            }
        } else if trimmed.starts_with("%% OXDRAW META") {
            // Parse: %% OXDRAW META path:<Path> commit:<Commit> diff_hash:<Hash>
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            for part in parts.iter().skip(3) {
                // Skip "%%", "OXDRAW", "META"
                if let Some(val) = part.strip_prefix("path:") {
                    metadata.path = Some(val.to_string());
                } else if let Some(val) = part.strip_prefix("commit:") {
                    metadata.commit = Some(val.to_string());
                } else if let Some(val) = part.strip_prefix("diff_hash:") {
                    metadata.diff_hash = val.parse().ok();
                }
            }
        }
    }
    (CodeMapMapping { nodes }, metadata)
}

pub fn serialize_codemap(
    mermaid: &str,
    mapping: &CodeMapMapping,
    metadata: &CodeMapMetadata,
) -> String {
    let mut output = mermaid.to_string();
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("\n");

    for (node_id, location) in &mapping.nodes {
        let mut parts = Vec::new();
        if let (Some(start), Some(end)) = (location.start_line, location.end_line) {
            parts.push(format!("line:{}-{}", start, end));
        }
        if let Some(symbol) = &location.symbol {
            parts.push(format!("def:{}", symbol));
        }

        let extra = if parts.is_empty() {
            String::new()
        } else {
            format!(" {}", parts.join(" "))
        };

        output.push_str(&format!(
            "%% OXDRAW CODE {} {}{}\n",
            node_id, location.file, extra
        ));
    }

    let mut meta_line = String::from("%% OXDRAW META");
    if let Some(path) = &metadata.path {
        meta_line.push_str(&format!(" path:{}", path));
    }
    if let Some(commit) = &metadata.commit {
        meta_line.push_str(&format!(" commit:{}", commit));
    }
    if let Some(diff_hash) = &metadata.diff_hash {
        meta_line.push_str(&format!(" diff_hash:{}", diff_hash));
    }
    output.push_str(&meta_line);
    output.push('\n');

    output
}

impl CodeMapMapping {
    pub fn resolve_symbols(&mut self, root: &Path) {
        let mut file_cache: HashMap<String, String> = HashMap::new();

        for location in self.nodes.values_mut() {
            // If we already have line numbers, we might want to verify them or just keep them.
            // But if we have a symbol and no lines (or we want to refresh), we resolve.
            // For now, let's prioritize the symbol if present.
            if let Some(symbol) = &location.symbol {
                if !file_cache.contains_key(&location.file) {
                    let file_path = root.join(&location.file);
                    if file_path.exists() {
                        if let Ok(content) = fs::read_to_string(&file_path) {
                            file_cache.insert(location.file.clone(), content);
                        }
                    }
                }

                if let Some(content) = file_cache.get(&location.file) {
                    if let Some((start, end)) =
                        find_symbol_definition(content, symbol, &location.file)
                    {
                        location.start_line = Some(start);
                        location.end_line = Some(end);
                    }
                }
            }
        }
    }
}

fn find_symbol_definition(content: &str, symbol: &str, file_path: &str) -> Option<(usize, usize)> {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Simple regex-based finder for now.
    // This is not perfect but covers many cases without heavy dependencies.

    let patterns = match ext {
        "rs" => vec![
            format!(r"fn\s+{}\b", regex::escape(symbol)),
            format!(r"struct\s+{}\b", regex::escape(symbol)),
            format!(r"enum\s+{}\b", regex::escape(symbol)),
            format!(r"trait\s+{}\b", regex::escape(symbol)),
            format!(r"mod\s+{}\b", regex::escape(symbol)),
            format!(r"type\s+{}\b", regex::escape(symbol)),
            format!(r"const\s+{}\b", regex::escape(symbol)),
        ],
        "ts" | "tsx" | "js" | "jsx" => vec![
            format!(r"function\s+{}\b", regex::escape(symbol)),
            format!(r"class\s+{}\b", regex::escape(symbol)),
            format!(r"interface\s+{}\b", regex::escape(symbol)),
            format!(r"type\s+{}\b", regex::escape(symbol)),
            format!(r"const\s+{}\s*=", regex::escape(symbol)),
            format!(r"let\s+{}\s*=", regex::escape(symbol)),
            format!(r"var\s+{}\s*=", regex::escape(symbol)),
        ],
        "py" => vec![
            format!(r"def\s+{}\b", regex::escape(symbol)),
            format!(r"class\s+{}\b", regex::escape(symbol)),
        ],
        "go" => vec![
            format!(r"func\s+{}\b", regex::escape(symbol)),
            format!(r"type\s+{}\b", regex::escape(symbol)),
        ],
        _ => vec![
            format!(r"{}\b", regex::escape(symbol)), // Fallback: just the name
        ],
    };

    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(&pattern) {
            if let Some(mat) = re.find(content) {
                // Found the start. Now try to estimate the end.
                // This is hard without a parser.
                // For now, let's just return the line where it starts, and maybe 10 lines after?
                // Or just the single line if we can't determine scope.

                let start_byte = mat.start();
                let start_line = content[..start_byte].lines().count() + 1;

                // Heuristic for end line: count braces?
                // This is very rough.
                let end_line = estimate_block_end(content, start_byte)
                    .map(|l| l + 1)
                    .unwrap_or(start_line);

                return Some((start_line, end_line));
            }
        }
    }

    None
}

fn estimate_block_end(content: &str, start_byte: usize) -> Option<usize> {
    let mut open_braces = 0;
    let mut found_brace = false;
    let mut lines = 0;
    let start_line_num = content[..start_byte].lines().count();

    for (_i, char) in content[start_byte..].char_indices() {
        if char == '{' {
            open_braces += 1;
            found_brace = true;
        } else if char == '}' {
            open_braces -= 1;
        }

        if char == '\n' {
            lines += 1;
        }

        if found_brace && open_braces == 0 {
            return Some(start_line_num + lines);
        }

        // Safety break for very long blocks or missing braces
        if lines > 500 {
            break;
        }
    }

    // If no braces found (e.g. Python), maybe look for indentation?
    // For now, fallback to just a few lines.
    if !found_brace {
        return Some(start_line_num + 5);
    }

    None
}

fn generate_deterministic_map(
    root_path: &Path,
    max_nodes: usize,
) -> Result<(String, CodeMapMapping)> {
    let mut nodes = HashMap::new();
    let mut edges = Vec::new();
    let mut symbol_to_node_id = HashMap::new();

    // 1. Scan files and find definitions
    let walker = WalkDir::new(root_path).into_iter();
    let include_exts = vec!["rs", "ts", "tsx", "js", "jsx", "py", "go"];
    let ignore_dirs = vec![
        "target",
        "node_modules",
        ".git",
        "dist",
        "build",
        ".next",
        "out",
    ];

    let mut files_content = HashMap::new();

    'outer: for entry in walker.filter_entry(|e| {
        let file_name = e.file_name().to_string_lossy();
        !ignore_dirs.iter().any(|d| file_name == *d)
    }) {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }

        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if include_exts.contains(&ext) {
                if let Ok(content) = fs::read_to_string(path) {
                    let rel_path = if root_path.is_file() {
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    } else {
                        path.strip_prefix(root_path)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string()
                    };
                    files_content.insert(rel_path.clone(), (content.clone(), ext.to_string()));

                    let defs = find_all_definitions(&content, ext);
                    for (symbol, start, end) in defs {
                        if nodes.len() >= max_nodes {
                            println!(
                                "Warning: Hit node limit ({}). Stopping scan to prevent huge diagrams.",
                                max_nodes
                            );
                            break 'outer;
                        }

                        let node_id = format!("node_{}", nodes.len());
                        nodes.insert(
                            node_id.clone(),
                            CodeLocation {
                                file: rel_path.clone(),
                                start_line: Some(start),
                                end_line: Some(end),
                                symbol: Some(symbol.clone()),
                            },
                        );
                        symbol_to_node_id.insert(symbol, node_id);
                    }
                }
            }
        }
    }

    // 2. Scan bodies for calls
    for (node_id, location) in &nodes {
        if location.symbol.is_some() {
            if let Some((content, _)) = files_content.get(&location.file) {
                let start_line = location.start_line.unwrap_or(0);
                let end_line = location.end_line.unwrap_or(content.lines().count());

                // Extract body content (approximate)
                let take_count = if end_line >= start_line {
                    end_line - start_line + 1
                } else {
                    0
                };

                let body: String = content
                    .lines()
                    .skip(start_line.saturating_sub(1))
                    .take(take_count)
                    .collect::<Vec<&str>>()
                    .join("\n");

                for (target_symbol, target_id) in &symbol_to_node_id {
                    if target_id == node_id {
                        continue;
                    } // Don't link to self

                    // Check if body contains target_symbol
                    if body.contains(target_symbol) {
                        // Verify with regex for word boundary
                        if let Ok(re) =
                            regex::Regex::new(&format!(r"\b{}\b", regex::escape(target_symbol)))
                        {
                            if re.is_match(&body) {
                                edges.push((node_id.clone(), target_id.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Generate Mermaid
    let mut mermaid = String::from("graph TD\n");
    for (id, location) in &nodes {
        let label = location.symbol.as_deref().unwrap_or("?");
        // Sanitize label for Mermaid
        let safe_label = label.replace("\"", "'").replace("[", "(").replace("]", ")");
        mermaid.push_str(&format!("    {}[{}]\n", id, safe_label));
    }

    // Deduplicate edges
    edges.sort();
    edges.dedup();

    for (from, to) in edges {
        mermaid.push_str(&format!("    {} --> {}\n", from, to));
    }

    Ok((mermaid, CodeMapMapping { nodes }))
}

fn generate_deterministic_uml_map(
    root_path: &Path,
    _max_nodes: usize,
) -> Result<(String, CodeMapMapping)> {
    let walker = WalkDir::new(root_path).into_iter();
    let include_exts = vec!["rs", "ts", "tsx", "js", "jsx", "py", "go"];
    let ignore_dirs = vec![
        "target",
        "node_modules",
        ".git",
        "dist",
        "build",
        ".next",
        "out",
    ];

    let mut file_data: HashMap<String, (String, String)> = HashMap::new();
    let mut symbol_locations: HashMap<String, CodeLocation> = HashMap::new();
    let mut node_ids_by_symbol: HashMap<String, String> = HashMap::new();
    let mut symbol_kinds: HashMap<String, String> = HashMap::new();
    let mut ordered_symbols: Vec<String> = Vec::new();

    for entry in walker.filter_entry(|e| {
        let file_name = e.file_name().to_string_lossy();
        !ignore_dirs.iter().any(|d| file_name == *d)
    }) {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }

        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !include_exts.contains(&ext) {
            continue;
        }

        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };

        let rel_path = if root_path.is_file() {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            path.strip_prefix(root_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
        };
        file_data.insert(rel_path.clone(), (content.clone(), ext.to_string()));

        for (symbol, start, end) in find_all_definitions(&content, ext) {
            if symbol_locations.contains_key(&symbol) {
                continue;
            }

            let node_id = format!("class_{}", symbol_locations.len());
            symbol_locations.insert(
                symbol.clone(),
                CodeLocation {
                    file: rel_path.clone(),
                    start_line: Some(start),
                    end_line: Some(end),
                    symbol: Some(symbol.clone()),
                },
            );
            node_ids_by_symbol.insert(symbol.clone(), node_id);
            ordered_symbols.push(symbol.clone());

            let kind = infer_symbol_kind(&content, ext, &symbol).unwrap_or_else(|| "type".to_string());
            symbol_kinds.insert(symbol, kind);
        }
    }

    let symbol_set: HashSet<String> = symbol_locations.keys().cloned().collect();
    let mut relationships: HashSet<(String, String, &'static str)> = HashSet::new();

    for (_file, (content, ext)) in &file_data {
        collect_language_relationships(content, ext, &symbol_set, &mut relationships);
    }

    let mut mapping_nodes: HashMap<String, CodeLocation> = HashMap::new();
    let mut mermaid = String::from("classDiagram\n");

    for symbol in &ordered_symbols {
        let Some(node_id) = node_ids_by_symbol.get(symbol) else {
            continue;
        };
        let Some(location) = symbol_locations.get(symbol) else {
            continue;
        };

        mapping_nodes.insert(node_id.clone(), location.clone());
        mermaid.push_str(&format!("    class {}\n", node_id));
        mermaid.push_str(&format!("    {} : {}\n", node_id, symbol));
        if let Some(kind) = symbol_kinds.get(symbol) {
            mermaid.push_str(&format!("    {} : <<{}>>\n", node_id, kind));
        }
    }

    let mut relationship_list: Vec<(String, String, &'static str)> = relationships
        .into_iter()
        .filter(|(lhs, rhs, _)| symbol_set.contains(lhs) && symbol_set.contains(rhs) && lhs != rhs)
        .collect();
    relationship_list.sort();

    for (lhs, rhs, rel) in relationship_list {
        let Some(lhs_id) = node_ids_by_symbol.get(&lhs) else {
            continue;
        };
        let Some(rhs_id) = node_ids_by_symbol.get(&rhs) else {
            continue;
        };
        mermaid.push_str(&format!("    {} {} {}\n", lhs_id, rel, rhs_id));
    }

    Ok((
        mermaid,
        CodeMapMapping {
            nodes: mapping_nodes,
        },
    ))
}

fn infer_symbol_kind(content: &str, ext: &str, symbol: &str) -> Option<String> {
    let escaped = regex::escape(symbol);
    let patterns: Vec<(&str, String)> = match ext {
        "rs" => vec![
            ("struct", format!(r"\bstruct\s+{}\b", escaped)),
            ("enum", format!(r"\benum\s+{}\b", escaped)),
            ("trait", format!(r"\btrait\s+{}\b", escaped)),
            ("function", format!(r"\bfn\s+{}\b", escaped)),
        ],
        "ts" | "tsx" | "js" | "jsx" => vec![
            ("class", format!(r"\bclass\s+{}\b", escaped)),
            ("interface", format!(r"\binterface\s+{}\b", escaped)),
            ("function", format!(r"\bfunction\s+{}\b", escaped)),
            ("type", format!(r"\btype\s+{}\b", escaped)),
        ],
        "py" => vec![
            ("class", format!(r"\bclass\s+{}\b", escaped)),
            ("function", format!(r"\bdef\s+{}\b", escaped)),
        ],
        "go" => vec![
            ("type", format!(r"\btype\s+{}\b", escaped)),
            ("function", format!(r"\bfunc\s+{}\b", escaped)),
        ],
        _ => vec![],
    };

    for (kind, pattern) in patterns {
        if let Ok(re) = regex::Regex::new(&pattern) {
            if re.is_match(content) {
                return Some(kind.to_string());
            }
        }
    }
    None
}

fn collect_language_relationships(
    content: &str,
    ext: &str,
    symbols: &HashSet<String>,
    out: &mut HashSet<(String, String, &'static str)>,
) {
    match ext {
        "ts" | "tsx" | "js" | "jsx" => {
            if let Ok(re_extends) = regex::Regex::new(r"class\s+(\w+)\s+extends\s+(\w+)") {
                for cap in re_extends.captures_iter(content) {
                    let derived = cap.get(1).map(|m| m.as_str().to_string());
                    let base = cap.get(2).map(|m| m.as_str().to_string());
                    if let (Some(derived), Some(base)) = (derived, base) {
                        if symbols.contains(&derived) && symbols.contains(&base) {
                            out.insert((base, derived, "<|--"));
                        }
                    }
                }
            }

            if let Ok(re_implements) =
                regex::Regex::new(r"class\s+(\w+)\s+implements\s+([A-Za-z0-9_,\s]+)")
            {
                for cap in re_implements.captures_iter(content) {
                    let class_name = cap.get(1).map(|m| m.as_str().to_string());
                    let interfaces = cap.get(2).map(|m| m.as_str().to_string());
                    let (Some(class_name), Some(interfaces)) = (class_name, interfaces) else {
                        continue;
                    };
                    if !symbols.contains(&class_name) {
                        continue;
                    }
                    for iface in interfaces.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        let iface_name = iface.to_string();
                        if symbols.contains(&iface_name) {
                            out.insert((iface_name, class_name.clone(), "<|.."));
                        }
                    }
                }
            }

            if let Ok(re_class_body) = regex::Regex::new(r"class\s+(\w+)[^{]*\{([\s\S]*?)\}") {
                let re_field = regex::Regex::new(r"\b\w+\??\s*:\s*([A-Z][A-Za-z0-9_]*)").ok();
                if let Some(re_field) = re_field {
                    for cap in re_class_body.captures_iter(content) {
                        let owner = cap.get(1).map(|m| m.as_str().to_string());
                        let body = cap.get(2).map(|m| m.as_str().to_string());
                        let (Some(owner), Some(body)) = (owner, body) else {
                            continue;
                        };
                        if !symbols.contains(&owner) {
                            continue;
                        }
                        for field_cap in re_field.captures_iter(&body) {
                            let Some(target) = field_cap.get(1).map(|m| m.as_str().to_string())
                            else {
                                continue;
                            };
                            if symbols.contains(&target) && target != owner {
                                out.insert((owner.clone(), target, "*--"));
                            }
                        }
                    }
                }
            }
        }
        "rs" => {
            if let Ok(re_impl_trait_for) = regex::Regex::new(r"impl\s+(\w+)\s+for\s+(\w+)") {
                for cap in re_impl_trait_for.captures_iter(content) {
                    let trait_name = cap.get(1).map(|m| m.as_str().to_string());
                    let impl_type = cap.get(2).map(|m| m.as_str().to_string());
                    if let (Some(trait_name), Some(impl_type)) = (trait_name, impl_type) {
                        if symbols.contains(&trait_name) && symbols.contains(&impl_type) {
                            out.insert((trait_name, impl_type, "<|.."));
                        }
                    }
                }
            }

            if let Ok(re_struct) = regex::Regex::new(r"struct\s+(\w+)\s*\{([\s\S]*?)\}") {
                let re_field_type = regex::Regex::new(r":\s*([A-Z][A-Za-z0-9_]*)").ok();
                if let Some(re_field_type) = re_field_type {
                    for cap in re_struct.captures_iter(content) {
                        let owner = cap.get(1).map(|m| m.as_str().to_string());
                        let body = cap.get(2).map(|m| m.as_str().to_string());
                        let (Some(owner), Some(body)) = (owner, body) else {
                            continue;
                        };
                        if !symbols.contains(&owner) {
                            continue;
                        }
                        for field_cap in re_field_type.captures_iter(&body) {
                            let Some(target) = field_cap.get(1).map(|m| m.as_str().to_string())
                            else {
                                continue;
                            };
                            if symbols.contains(&target) && target != owner {
                                out.insert((owner.clone(), target, "*--"));
                            }
                        }
                    }
                }
            }
        }
        "py" => {
            if let Ok(re_py_inherit) = regex::Regex::new(r"class\s+(\w+)\((\w+)\)\s*:") {
                for cap in re_py_inherit.captures_iter(content) {
                    let derived = cap.get(1).map(|m| m.as_str().to_string());
                    let base = cap.get(2).map(|m| m.as_str().to_string());
                    if let (Some(derived), Some(base)) = (derived, base) {
                        if symbols.contains(&derived) && symbols.contains(&base) && base != "object" {
                            out.insert((base, derived, "<|--"));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn find_all_definitions(content: &str, ext: &str) -> Vec<(String, usize, usize)> {
    let mut defs = Vec::new();

    let patterns = match ext {
        "rs" => vec![
            r"fn\s+(\w+)",
            r"struct\s+(\w+)",
            r"enum\s+(\w+)",
            r"trait\s+(\w+)",
            r"mod\s+(\w+)",
        ],
        "ts" | "tsx" | "js" | "jsx" => vec![
            r"function\s+(\w+)",
            r"class\s+(\w+)",
            r"interface\s+(\w+)",
            r"const\s+(\w+)\s*=",
            r"let\s+(\w+)\s*=",
        ],
        "py" => vec![r"def\s+(\w+)", r"class\s+(\w+)"],
        "go" => vec![r"func\s+(\w+)", r"type\s+(\w+)"],
        _ => vec![],
    };

    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for cap in re.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    let symbol = m.as_str().to_string();
                    let start_byte = m.start();
                    let start_line = content[..start_byte].lines().count() + 1; // 1-based
                    let end_line = estimate_block_end(content, start_byte)
                        .map(|l| l + 1)
                        .unwrap_or(start_line);
                    defs.push((symbol, start_line, end_line));
                }
            }
        }
    }

    defs
}
