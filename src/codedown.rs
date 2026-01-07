use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::codemap::{CodeLocation, CodeMapMapping, CodeMapMetadata};

#[derive(Debug, Serialize, Deserialize)]
struct LlmResponse {
    markdown: String,
    mapping: HashMap<String, CodeLocation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    commit: String,
    diff_hash: u64,
    markdown: String,
    mapping: CodeMapMapping,
}

pub enum CodedownStyle {
    Architecture,
    Tutorial,
    Api,
}

impl CodedownStyle {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "architecture" => Some(CodedownStyle::Architecture),
            "tutorial" => Some(CodedownStyle::Tutorial),
            "api" => Some(CodedownStyle::Api),
            _ => None,
        }
    }
}

/// Extract codedown mappings from markdown with HTML comments
pub fn extract_codedown_mappings(source: &str) -> (CodeMapMapping, CodeMapMetadata) {
    let mut nodes = HashMap::new();
    let mut metadata = CodeMapMetadata {
        path: None,
        commit: None,
        diff_hash: None,
    };

    // Look for <!-- OXDRAW MAPPING ... --> comment
    if let Some(start) = source.find("<!-- OXDRAW MAPPING") {
        if let Some(end) = source[start..].find("-->") {
            let comment_content = &source[start + "<!-- OXDRAW MAPPING".len()..start + end];
            // Parse JSON from comment
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(comment_content.trim()) {
                if let Some(nodes_obj) = parsed.get("nodes").and_then(|n| n.as_object()) {
                    for (node_id, location) in nodes_obj {
                        if let Ok(loc) = serde_json::from_value::<CodeLocation>(location.clone()) {
                            nodes.insert(node_id.clone(), loc);
                        }
                    }
                }
            }
        }
    }

    // Look for <!-- OXDRAW META ... --> comment
    if let Some(start) = source.find("<!-- OXDRAW META") {
        if let Some(end) = source[start..].find("-->") {
            let meta_content = &source[start + "<!-- OXDRAW META".len()..start + end];
            let parts: Vec<&str> = meta_content.split_whitespace().collect();
            for part in parts {
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

/// Serialize codedown with mappings as HTML comments
pub fn serialize_codedown(
    markdown: &str,
    mapping: &CodeMapMapping,
    metadata: &CodeMapMetadata,
) -> String {
    let mut output = markdown.to_string();

    // Remove existing OXDRAW comments if present
    if let Some(start) = output.find("<!-- OXDRAW MAPPING") {
        if let Some(end) = output[start..].find("-->") {
            let full_end = start + end + 3; // +3 for "-->""
            output.replace_range(start..full_end, "");
        }
    }
    if let Some(start) = output.find("<!-- OXDRAW META") {
        if let Some(end) = output[start..].find("-->") {
            let full_end = start + end + 3;
            output.replace_range(start..full_end, "");
        }
    }

    // Trim trailing whitespace
    output = output.trim_end().to_string();

    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("\n");

    // Add mapping comment
    let mapping_json = serde_json::json!({
        "nodes": mapping.nodes
    });
    output.push_str("<!-- OXDRAW MAPPING\n");
    output.push_str(&serde_json::to_string_pretty(&mapping_json).unwrap());
    output.push_str("\n-->\n");

    // Add metadata comment
    let mut meta_parts = Vec::new();
    if let Some(path) = &metadata.path {
        meta_parts.push(format!("path:{}", path));
    }
    if let Some(commit) = &metadata.commit {
        meta_parts.push(format!("commit:{}", commit));
    }
    if let Some(diff_hash) = &metadata.diff_hash {
        meta_parts.push(format!("diff_hash:{}", diff_hash));
    }

    if !meta_parts.is_empty() {
        output.push_str(&format!("<!-- OXDRAW META {} -->\n", meta_parts.join(" ")));
    }

    output
}

fn get_git_info(path: &Path) -> Option<(String, u64, PathBuf)> {
    use std::process::Command;

    let repo_root = Command::new("git")
        .args(&["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))?;

    let commit = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())?;

    let diff_output = Command::new("git")
        .args(&["diff", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let mut hasher = DefaultHasher::new();
    diff_output.hash(&mut hasher);
    let diff_hash = hasher.finish();

    Some((commit, diff_hash, repo_root))
}

/// Generate a codedown from a codebase using AI
pub async fn generate_codedown(
    path: &Path,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
    regen: bool,
    custom_prompt: Option<String>,
    style: CodedownStyle,
    gemini_key: Option<String>,
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
    let cache_path = config_dir.join(format!("codedown_cache_{:x}.json", path_hash));

    // Check cache
    if !regen {
        if let Some((commit, diff_hash, _)) = &git_info {
            if let Ok(cache_content) = fs::read_to_string(&cache_path) {
                if let Ok(cache) = serde_json::from_str::<CacheEntry>(&cache_content) {
                    if cache.commit == *commit && cache.diff_hash == *diff_hash {
                        println!(
                            "Using cached codedown for commit {} (diff hash: {:x})",
                            commit, diff_hash
                        );
                        return Ok((cache.markdown, cache.mapping));
                    }
                }
            }
        }
    }

    println!("Generating codedown with AI...");

    // Scan codebase
    let (file_summaries, _) = scan_codebase(path)?;

    // Build prompt based on style
    let base_prompt = match style {
        CodedownStyle::Architecture => {
            "You are a technical documentation expert. Analyze the following codebase and generate a \
            comprehensive markdown document that explains the architecture, key components, and data flow.\n\n\
            For EACH LINE in your markdown that references code, you MUST provide a mapping.\n\n"
        }
        CodedownStyle::Tutorial => {
            "You are a technical documentation expert. Analyze the following codebase and generate a \
            step-by-step tutorial-style markdown document that explains how the codebase works.\n\n\
            For EACH LINE in your markdown that references code, you MUST provide a mapping.\n\n"
        }
        CodedownStyle::Api => {
            "You are a technical documentation expert. Analyze the following codebase and generate a \
            markdown document that documents the public APIs, functions, classes, and interfaces.\n\n\
            For EACH LINE in your markdown that references code, you MUST provide a mapping.\n\n"
        }
    };

    let prompt = if let Some(custom) = custom_prompt {
        format!(
            "{}\n\
            Custom instructions: {}\n\n\
            Return ONLY a JSON object with this structure:\n\
            {{\n\
                \"markdown\": \"# Title\\n\\nContent...\\n\",\n\
                \"mapping\": {{\n\
                    \"line_1\": {{ \"file\": \"src/main.rs\", \"symbol\": \"main\", \"start_line\": 10, \"end_line\": 20 }},\n\
                    \"line_3\": {{ \"file\": \"src/lib.rs\", \"start_line\": 5, \"end_line\": 15 }}\n\
                }}\n\
            }}\n\n\
            Here are the files:\n\n{}",
            base_prompt,
            custom,
            file_summaries.join("\n\n")
        )
    } else {
        format!(
            "{}\
            Return ONLY a JSON object with this structure:\n\
            {{\n\
                \"markdown\": \"# Title\\n\\nContent...\\n\",\n\
                \"mapping\": {{\n\
                    \"line_1\": {{ \"file\": \"src/main.rs\", \"symbol\": \"main\", \"start_line\": 10, \"end_line\": 20 }},\n\
                    \"line_3\": {{ \"file\": \"src/lib.rs\", \"start_line\": 5, \"end_line\": 15 }}\n\
                }}\n\
            }}\n\n\
            Here are the files:\n\n{}",
            base_prompt,
            file_summaries.join("\n\n")
        )
    };

    // Call AI (reuse logic from codemap)
    let (markdown, mapping) =
        call_ai_for_codedown(&prompt, api_key, model, api_url, gemini_key).await?;

    // Cache the result
    if let Some((commit, diff_hash, _)) = git_info {
        let cache_entry = CacheEntry {
            commit,
            diff_hash,
            markdown: markdown.clone(),
            mapping: CodeMapMapping {
                nodes: mapping.nodes.clone(),
            },
        };
        if let Ok(json) = serde_json::to_string_pretty(&cache_entry) {
            let _ = fs::write(cache_path, json);
        }
    }

    Ok((markdown, mapping))
}

/// Augment existing markdown with mappings
pub async fn augment_markdown_with_mappings(
    markdown_path: &Path,
    repo_path: &Path,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
    gemini_key: Option<String>,
) -> Result<(String, CodeMapMapping)> {
    println!("Augmenting markdown with code mappings...");

    // Read existing markdown
    let markdown_content =
        fs::read_to_string(markdown_path).context("Failed to read markdown file")?;

    // Scan codebase
    let (file_summaries, _) = scan_codebase(repo_path)?;

    // Build prompt
    let prompt = format!(
        "You are analyzing a markdown document about a codebase. For each line in the markdown, \
        determine which file and line range it refers to (if any).\n\n\
        Return a mapping from markdown line numbers to code locations.\n\n\
        Markdown content:\n{}\n\n\
        Codebase files:\n{}\n\n\
        Return ONLY JSON:\n\
        {{\n\
            \"mapping\": {{\n\
                \"line_5\": {{ \"file\": \"src/main.rs\", \"start_line\": 10, \"end_line\": 20 }},\n\
                \"line_12\": {{ \"file\": \"src/lib.rs\", \"symbol\": \"MyStruct\" }}\n\
            }}\n\
        }}",
        markdown_content,
        file_summaries.join("\n\n")
    );

    // Call AI
    let response_text = call_ai(&prompt, api_key, model, api_url, gemini_key).await?;

    let clean_text = response_text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // Parse response
    let parsed: serde_json::Value =
        serde_json::from_str(clean_text).context("Failed to parse AI response as JSON")?;

    let mapping_obj = parsed
        .get("mapping")
        .and_then(|m| m.as_object())
        .ok_or_else(|| anyhow!("No mapping found in response"))?;

    let mut nodes = HashMap::new();
    for (line_id, location) in mapping_obj {
        if let Ok(loc) = serde_json::from_value::<CodeLocation>(location.clone()) {
            nodes.insert(line_id.clone(), loc);
        }
    }

    let mapping = CodeMapMapping { nodes };

    Ok((markdown_content, mapping))
}

async fn call_ai_for_codedown(
    prompt: &str,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
    gemini_key: Option<String>,
) -> Result<(String, CodeMapMapping)> {
    let mut last_error = String::new();

    for attempt in 0..4 {
        if attempt > 0 {
            println!("Retry attempt {} of 3...", attempt);
        }

        let enhanced_prompt = if attempt == 0 {
            prompt.to_string()
        } else {
            format!(
                "{}\n\nPrevious attempt failed with error: {}\nPlease fix the issue and try again.",
                prompt, last_error
            )
        };

        let response_text = match call_ai(
            &enhanced_prompt,
            api_key.clone(),
            model.clone(),
            api_url.clone(),
            gemini_key.clone(),
        )
        .await
        {
            Ok(text) => text,
            Err(e) => {
                last_error = e.to_string();
                continue;
            }
        };

        let clean_text = response_text.trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        // Parse JSON response
        match serde_json::from_str::<LlmResponse>(clean_text) {
            Ok(llm_response) => {
                // Validate
                if llm_response.markdown.is_empty() {
                    last_error = "Empty markdown content".to_string();
                    continue;
                }

                let mapping = CodeMapMapping {
                    nodes: llm_response.mapping,
                };
                return Ok((llm_response.markdown, mapping));
            }
            Err(e) => {
                last_error = format!("Failed to parse JSON: {}", e);
                continue;
            }
        }
    }

    bail!(
        "Failed to generate codedown after 4 attempts. Last error: {}",
        last_error
    )
}

async fn call_ai(
    prompt: &str,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
    gemini_key: Option<String>,
) -> Result<String> {
    // Use Gemini if key provided
    if let Some(key) = gemini_key {
        return call_gemini(prompt, &key, model.as_deref()).await;
    }

    // Otherwise use OpenAI-compatible API
    let api_key = api_key.ok_or_else(|| {
        anyhow!("No API key provided. Set OPENAI_API_KEY or use --api-key or --gemini")
    })?;
    let api_url =
        api_url.unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());
    let model = model.unwrap_or_else(|| "gpt-4".to_string());

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.7
    });

    let response = client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to API")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        bail!("API request failed with status {}: {}", status, error_text);
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse API response")?;

    let content = response_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow!("Unexpected API response format"))?;

    Ok(content.to_string())
}

async fn call_gemini(prompt: &str, api_key: &str, model: Option<&str>) -> Result<String> {
    let model = model.unwrap_or("gemini-2.5-flash");
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "contents": [{
            "parts": [{"text": prompt}]
        }]
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to Gemini API")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        bail!(
            "Gemini API request failed with status {}: {}",
            status,
            error_text
        );
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Gemini response")?;

    let content = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Unexpected Gemini API response format"))?;

    Ok(content.to_string())
}

fn scan_codebase(path: &Path) -> Result<(Vec<String>, String)> {
    use walkdir::WalkDir;

    let mut file_summaries = Vec::new();
    let mut total_chars = 0;
    const MAX_TOTAL_CHARS: usize = 100_000;
    const MAX_FILE_CHARS: usize = 10_000;

    // Common patterns to skip
    let skip_patterns = [
        "node_modules",
        "target",
        ".git",
        "dist",
        "build",
        ".next",
        "vendor",
        "__pycache__",
        ".venv",
    ];

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !skip_patterns.iter().any(|p| name.contains(p))
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        // Only include source code files
        if !matches!(
            ext,
            "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        ) {
            continue;
        }

        if let Ok(content) = fs::read_to_string(path) {
            let relative_path = path
                .strip_prefix(path.parent().unwrap_or(path))
                .unwrap_or(path)
                .display()
                .to_string();

            let truncated = if content.len() > MAX_FILE_CHARS {
                format!("{}... (truncated)", &content[..MAX_FILE_CHARS])
            } else {
                content
            };

            let summary = format!("File: {}\n{}", relative_path, truncated);
            total_chars += summary.len();

            if total_chars > MAX_TOTAL_CHARS {
                break;
            }

            file_summaries.push(summary);
        }
    }

    let granularity = if file_summaries.len() == 1 {
        "file"
    } else if path.join(".git").exists() {
        "repo"
    } else {
        "directory"
    };

    Ok((file_summaries, granularity.to_string()))
}
