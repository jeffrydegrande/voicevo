use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Request body for the Anthropic Messages API.
#[derive(Serialize)]
struct Request {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

/// Response from the Anthropic Messages API.
/// We only parse the fields we need.
#[derive(Deserialize)]
struct Response {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

/// Anthropic error response shape.
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
        max_tokens: 1024,
        system: system.to_string(),
        messages: vec![Message {
            role: "user".into(),
            content: user_message.into(),
        }],
    };

    let client = reqwest::Client::new();
    let response = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .context("Failed to send request to Anthropic API")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read Anthropic API response")?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
            anyhow::bail!("Anthropic API error ({}): {}", status, err.error.message);
        }
        anyhow::bail!("Anthropic API error ({}): {}", status, body);
    }

    let parsed: Response = serde_json::from_str(&body)
        .context("Failed to parse Anthropic API response")?;

    let text = parsed
        .content
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("");

    Ok(text)
}

/// Call the Anthropic Messages API and return the text response.
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
