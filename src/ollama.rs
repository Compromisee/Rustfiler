use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;

// ─────────────────────────────────────────────────────────────────────────────
// Public data types  (all Clone so AppState can clone them)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClassification {
    pub category: String,
    pub subcategory: String,
    pub suggested_name: String,
    pub project_hint: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub group_name: String,
    pub category_folder: String,
    pub files: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Ollama HTTP client
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OllamaClient {
    client: reqwest::Client,
    url: String,
    model: String,
    /// Limit concurrent in-flight requests so we don't overwhelm Ollama.
    semaphore: Arc<Semaphore>,
}

// ── wire types ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
    options: GenerateOptions,
}

#[derive(Serialize)]
struct GenerateOptions {
    temperature: f32,
    num_predict: i32,
    top_p: f32,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

// ── impl ─────────────────────────────────────────────────────────────────────

impl OllamaClient {
    pub fn new(url: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .pool_max_idle_per_host(10)
                .build()
                .expect("Failed to build HTTP client"),
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            semaphore: Arc::new(Semaphore::new(4)),
        }
    }

    // ── health ────────────────────────────────────────────────────────────────

    pub async fn health_check(&self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}/api/tags", self.url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Ollama returned status: {}", resp.status())
        }
    }

    // ── raw generation ────────────────────────────────────────────────────────

    pub async fn generate(&self, prompt: &str) -> Result<String> {
        let _permit = self.semaphore.acquire().await?;

        let req = GenerateRequest {
            model: self.model.clone(),
            prompt: prompt.to_string(),
            stream: false,
            options: GenerateOptions {
                temperature: 0.1,
                num_predict: 100,
                top_p: 0.9,
            },
        };

        let resp = self
            .client
            .post(format!("{}/api/generate", self.url))
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama error {}: {}", status, body);
        }

        let gen: GenerateResponse = resp.json().await?;
        Ok(gen.response.trim().to_string())
    }

    // ── classify ──────────────────────────────────────────────────────────────

    pub async fn classify_file(
        &self,
        filename: &str,
        content_preview: &str,
        extension: &str,
    ) -> Result<FileClassification> {
        let prompt = format!(
            r#"You are a file classifier. Analyze this file and respond with ONLY a JSON object, no other text.

Filename: {filename}
Extension: {extension}
Content preview (first 500 chars):
---
{content_preview}
---

Respond with EXACTLY this JSON format:
{{"category": "<one of: coding_project, documents, images, music, videos, archives, data, config, scripts, web, misc>", "subcategory": "<more specific type>", "suggested_name": "<better filename or same if good>", "project_hint": "<if part of a coding project describe it briefly, otherwise empty string>", "confidence": <0.0-1.0>}}

Rules:
- category must be one of the listed options exactly
- suggested_name must be a valid filename with extension
- Keep suggested_name same as original if it is already descriptive
- project_hint should identify programming language/framework if coding related"#
        );

        let response = self.generate(&prompt).await?;
        parse_classification(&response, filename)
    }

    // ── rename suggestion ─────────────────────────────────────────────────────

    pub async fn suggest_rename(
        &self,
        filename: &str,
        content_preview: &str,
    ) -> Result<String> {
        let prompt = format!(
            r#"You are a file renamer. Given this file, suggest a better descriptive filename.

Current filename: {filename}
Content preview:
---
{content_preview}
---

Rules:
- Respond with ONLY the new filename (with extension), nothing else
- Keep it concise (max 50 chars)
- Use snake_case
- Keep the same extension
- If the current name is already good, return it unchanged
- Only use alphanumeric chars, underscores, hyphens, and dots"#
        );

        let response = self.generate(&prompt).await?;
        let suggested = response.trim().trim_matches('"').trim().to_string();

        if suggested.is_empty()
            || suggested.contains('/')
            || suggested.contains('\\')
            || suggested.len() > 100
        {
            Ok(filename.to_string())
        } else {
            Ok(suggested)
        }
    }

    // ── group detection ───────────────────────────────────────────────────────

    pub async fn detect_project_groups(
        &self,
        files_description: &str,
    ) -> Result<Vec<ProjectGroup>> {
        let prompt = format!(
            r#"You are analyzing files in a directory. Group related files into projects.

Files:
{files_description}

Respond with ONLY a JSON array. Each element:
{{"group_name": "<descriptive_name>", "category_folder": "<one of: coding_projects, documents, scripts, web_projects, data_files, config_files, misc>", "files": ["filename1", "filename2"]}}

Rules:
- Group related files together (same project, same topic)
- coding_projects for full programming projects
- Use snake_case for group_name
- Every file must appear in exactly one group
- Single unrelated files go in a group by themselves with category misc"#
        );

        let response = self.generate(&prompt).await?;
        parse_project_groups(&response)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON parsing helpers
// ─────────────────────────────────────────────────────────────────────────────

fn parse_classification(response: &str, original_filename: &str) -> Result<FileClassification> {
    let json_str = extract_json_object(response);

    match serde_json::from_str::<FileClassification>(&json_str) {
        Ok(mut fc) => {
            if fc.suggested_name.is_empty() {
                fc.suggested_name = original_filename.to_string();
            }
            if !(0.0..=1.0).contains(&fc.confidence) {
                fc.confidence = 0.5;
            }
            let valid = [
                "coding_project",
                "documents",
                "images",
                "music",
                "videos",
                "archives",
                "data",
                "config",
                "scripts",
                "web",
                "misc",
            ];
            if !valid.contains(&fc.category.as_str()) {
                fc.category = "misc".to_string();
            }
            Ok(fc)
        }
        Err(_) => Ok(FileClassification {
            category: "misc".to_string(),
            subcategory: "unknown".to_string(),
            suggested_name: original_filename.to_string(),
            project_hint: String::new(),
            confidence: 0.1,
        }),
    }
}

fn parse_project_groups(response: &str) -> Result<Vec<ProjectGroup>> {
    let json_str = extract_json_array(response);
    match serde_json::from_str::<Vec<ProjectGroup>>(&json_str) {
        Ok(groups) => Ok(groups),
        Err(_) => Ok(vec![]),
    }
}

fn extract_json_object(text: &str) -> String {
    let mut depth = 0i32;
    let mut start = None;
    let mut end = None;

    for (i, ch) in text.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => text[s..e].to_string(),
        _ => text.to_string(),
    }
}

fn extract_json_array(text: &str) -> String {
    let mut depth = 0i32;
    let mut start = None;
    let mut end = None;

    for (i, ch) in text.char_indices() {
        match ch {
            '[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => text[s..e].to_string(),
        _ => text.to_string(),
    }
}