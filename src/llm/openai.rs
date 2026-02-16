use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.openai.com/v1/responses";

/// Request body for the OpenAI Responses API.
#[derive(Serialize)]
struct Request {
    model: String,
    instructions: String,
    input: String,
}

/// Response from the OpenAI Responses API.
#[derive(Deserialize)]
struct Response {
    /// Top-level convenience field (may or may not be present).
    output_text: Option<String>,
    /// The output array — fallback for extracting text.
    #[serde(default)]
    output: Vec<OutputItem>,
}

#[derive(Deserialize)]
struct OutputItem {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

/// OpenAI error response shape.
#[derive(Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

/// Async implementation — used directly when we already have a runtime.
pub async fn complete_async(
    api_key: &str,
    model: &str,
    system: &str,
    user_message: &str,
) -> Result<String> {
    let request = Request {
        model: model.to_string(),
        instructions: system.to_string(),
        input: user_message.to_string(),
    };

    let client = reqwest::Client::new();
    let response = client
        .post(API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .context("Failed to send request to OpenAI API")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read OpenAI API response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
            anyhow::bail!("OpenAI API error ({}): {}", status, err.error.message);
        }
        anyhow::bail!("OpenAI API error ({}): {}", status, body);
    }

    let parsed: Response = serde_json::from_str(&body)
        .context("Failed to parse OpenAI API response")?;

    // Try the top-level convenience field first
    if let Some(text) = &parsed.output_text {
        if !text.is_empty() {
            return Ok(text.clone());
        }
    }

    // Fall back to extracting from the output array
    for item in &parsed.output {
        for block in &item.content {
            if block.block_type == "output_text" && !block.text.is_empty() {
                return Ok(block.text.clone());
            }
        }
    }

    anyhow::bail!(
        "OpenAI returned no text. Raw response: {}",
        &body[..body.len().min(500)]
    );
}

/// Call the OpenAI Responses API and return the text response.
pub fn complete(
    api_key: &str,
    model: &str,
    system: &str,
    user_message: &str,
) -> Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create async runtime")?;

    rt.block_on(complete_async(api_key, model, system, user_message))
}
