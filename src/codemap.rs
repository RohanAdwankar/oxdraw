use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;
use directories::ProjectDirs;

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
) -> Result<(String, CodeMapMapping)> {
    let git_info = get_git_info(path);
    
    let project_dirs = ProjectDirs::from("", "", "oxdraw")
        .ok_or_else(|| anyhow!("Could not determine config directory"))?;
    let config_dir = project_dirs.config_dir();
    fs::create_dir_all(config_dir).context("Failed to create config directory")?;

    let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    abs_path.hash(&mut hasher);
    let path_hash = hasher.finish();
    let cache_path = config_dir.join(format!("cache_{:x}.json", path_hash));

    if !regen {
        if let Some((commit, diff_hash, _)) = &git_info {
            if let Ok(cache_content) = fs::read_to_string(&cache_path) {
                if let Ok(cache) = serde_json::from_str::<CacheEntry>(&cache_content) {
                    if cache.commit == *commit && cache.diff_hash == *diff_hash {
                        println!("Using cached code map for commit {} (diff hash: {:x})", commit, diff_hash);
                        return Ok((cache.mermaid, cache.mapping));
                    }
                }
            }
        }
    }

    println!("Scanning codebase at {}...", path.display());
    let (file_summaries, granularity) = scan_codebase(path)?;
    
    println!("Found {} files. Generating code map...", file_summaries.len());
    
    let base_prompt = match granularity {
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
    };

    let mut prompt = format!(
        "{}
        
        Return ONLY a JSON object with the following structure. Do not include other components of mermaid syntax such as as style This is the JSON schema to follow:
        {{
            \"mermaid\": \"graph TD\\n    A[Node Label] --> B[Another Node]\",
            \"mapping\": {{
                \"A\": {{ \"file\": \"src/main.rs\", \"symbol\": \"main\", \"start_line\": 10, \"end_line\": 20 }},
                \"B\": {{ \"file\": \"src/lib.rs\", \"symbol\": \"MyStruct\", \"start_line\": 5, \"end_line\": 15 }}
            }}
        }}
        ", base_prompt
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
    let url = api_url.unwrap_or_else(|| "http://localhost:8080/v1/responses".to_string());
    let model = model.unwrap_or_else(|| "gemini-2.0-flash".to_string());
    
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 4;

    loop {
        attempts += 1;
        if attempts > MAX_ATTEMPTS {
             bail!("Failed to generate valid code map after {} attempts", MAX_ATTEMPTS);
        }

        if attempts > 1 {
            println!("Attempt {}/{}...", attempts, MAX_ATTEMPTS);
        }

        let mut body = HashMap::new();
        body.insert("model", model.clone());
        body.insert("input", prompt.clone());

        let mut request = client.post(&url)
            .json(&body);

        if let Some(key) = &api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let response = request.send().await.context("Failed to send request to LLM")?;
        
        if !response.status().is_success() {
            let text = response.text().await?;
            return Err(anyhow!("LLM API returned error: {}", text));
        }

        let response_json: serde_json::Value = response.json().await.context("Failed to parse LLM response JSON")?;
        
        // Try to extract text from different possible formats
        let output_text = if let Some(text) = response_json.get("output_text").and_then(|v| v.as_str()) {
            text.to_string()
        } else if let Some(choices) = response_json.get("choices").and_then(|v| v.as_array()) {
            // Standard OpenAI format
            choices.first()
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
        let clean_json = output_text.trim()
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
                        mapping: CodeMapMapping { nodes: result.mapping.clone() },
                    };
                    if let Ok(json) = serde_json::to_string_pretty(&cache_entry) {
                        let _ = fs::write(cache_path, json);
                    }
                }
                return Ok((result.mermaid, CodeMapMapping { nodes: result.mapping }));
            },
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
    let diagram = Diagram::parse(&response.mermaid).context("Failed to parse generated Mermaid diagram")?;

    // 2. Check Mapping Completeness
    for node_id in diagram.nodes.keys() {
        if !response.mapping.contains_key(node_id) {
            bail!("Node '{}' is present in the diagram but missing from the mapping object.", node_id);
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
                bail!("Node '{}' is isolated (not connected to any other node). All nodes must be connected.", node_id);
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
    let root_str = String::from_utf8_lossy(&root_output.stdout).trim().to_string();
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
    let include_exts = vec!["rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cpp", "h"];
    let ignore_dirs = vec!["target", "node_modules", ".git", "dist", "build", ".next", "out"];

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
                    let rel_path = path.strip_prefix(root_path).unwrap_or(path).to_string_lossy();
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
    let mut metadata = CodeMapMetadata { path: None, commit: None, diff_hash: None };

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
                
                nodes.insert(node_id, CodeLocation {
                    file: file_path,
                    start_line,
                    end_line,
                    symbol,
                });
            }
        } else if trimmed.starts_with("%% OXDRAW META") {
             // Parse: %% OXDRAW META path:<Path> commit:<Commit> diff_hash:<Hash>
             let parts: Vec<&str> = trimmed.split_whitespace().collect();
             for part in parts.iter().skip(3) { // Skip "%%", "OXDRAW", "META"
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

pub fn serialize_codemap(mermaid: &str, mapping: &CodeMapMapping, metadata: &CodeMapMetadata) -> String {
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
        
        output.push_str(&format!("%% OXDRAW CODE {} {}{}\n", node_id, location.file, extra));
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
        for location in self.nodes.values_mut() {
            // If we already have line numbers, we might want to verify them or just keep them.
            // But if we have a symbol and no lines (or we want to refresh), we resolve.
            // For now, let's prioritize the symbol if present.
            if let Some(symbol) = &location.symbol {
                let file_path = root.join(&location.file);
                if file_path.exists() {
                    if let Ok(content) = fs::read_to_string(&file_path) {
                        if let Some((start, end)) = find_symbol_definition(&content, symbol, &location.file) {
                            location.start_line = Some(start);
                            location.end_line = Some(end);
                        }
                    }
                }
            }
        }
    }
}

fn find_symbol_definition(content: &str, symbol: &str, file_path: &str) -> Option<(usize, usize)> {
    let ext = Path::new(file_path).extension().and_then(|s| s.to_str()).unwrap_or("");
    
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
                let start_line = content[..start_byte].lines().count();
                
                // Heuristic for end line: count braces?
                // This is very rough.
                let end_line = estimate_block_end(content, start_byte).unwrap_or(start_line);
                
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
