use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeMapMapping {
    pub nodes: HashMap<String, CodeLocation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeLocation {
    pub file: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LlmResponse {
    mermaid: String,
    mapping: HashMap<String, CodeLocation>,
}

pub async fn generate_code_map(
    path: &Path,
    api_key: Option<String>,
    model: Option<String>,
    api_url: Option<String>,
) -> Result<(String, CodeMapMapping)> {
    println!("Scanning codebase at {}...", path.display());
    let file_summaries = scan_codebase(path)?;
    
    println!("Found {} files. Generating code map...", file_summaries.len());
    
    let prompt = format!(
        "You are an expert software architect. Analyze the following codebase and generate a Mermaid flowchart that explains the high-level architecture and data flow.
        
        For each node in the diagram, you MUST provide a mapping to the specific file and line numbers that the node represents.
        IMPORTANT: The keys in the 'mapping' object MUST match exactly the node IDs used in the Mermaid diagram.
        
        Return ONLY a JSON object with the following structure:
        {{
            \"mermaid\": \"graph TD\\n    A[Node Label] --> B[Another Node]\",
            \"mapping\": {{
                \"A\": {{ \"file\": \"src/main.rs\", \"start_line\": 10, \"end_line\": 20 }},
                \"B\": {{ \"file\": \"src/lib.rs\", \"start_line\": 5, \"end_line\": 15 }}
            }}
        }}

        Here are the files:
        
        {}
        ",
        file_summaries.join("\n\n")
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let url = api_url.unwrap_or_else(|| "http://localhost:8080/v1/responses".to_string());
    let model = model.unwrap_or_else(|| "gemini-2.0-flash".to_string());
    
    // ...existing code...
    
    let mut body = HashMap::new();
    body.insert("model", model);
    body.insert("input", prompt);

    let mut request = client.post(&url)
        .json(&body);

    if let Some(key) = api_key {
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
        // Fallback for the custom format in the issue which has "output": [ { "content": [ { "text": "..." } ] } ]
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

    let result: LlmResponse = serde_json::from_str(clean_json).context("Failed to parse generated JSON from LLM")?;

    Ok((result.mermaid, CodeMapMapping { nodes: result.mapping }))
}

fn scan_codebase(root_path: &Path) -> Result<Vec<String>> {
    let mut summaries = Vec::new();
    let mut total_chars = 0;
    const MAX_TOTAL_CHARS: usize = 100_000; // Limit total context size
    
    // Basic ignore list
    let ignore_dirs = vec!["target", "node_modules", ".git", "dist", "build", ".next"];
    let include_exts = vec!["rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "c", "cpp", "h"];

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
                    let truncated = if content.len() > 5000 {
                        format!("{}... (truncated)", &content[..5000])
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
    
    Ok(summaries)
}
