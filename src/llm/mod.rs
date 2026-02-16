mod anthropic;
mod openai;
pub mod prompt;
pub mod provider;

use anyhow::{Context, Result};
use provider::Provider;

use crate::storage::session_data::SessionData;

/// Run the full LLM interpretation pipeline:
///   1. Build prompts from session data
///   2. Resolve API key and model
///   3. Call the appropriate provider
///   4. Return the interpretation text
pub fn interpret(
    provider: &Provider,
    model: Option<&str>,
    current: &SessionData,
    history: &[SessionData],
) -> Result<String> {
    let api_key = provider.api_key()?;
    let model = model.unwrap_or_else(|| provider.default_model());

    let system = prompt::system_prompt();
    let user = prompt::user_prompt(current, history);

    match provider {
        Provider::Anthropic => anthropic::complete(&api_key, model, &system, &user),
        Provider::OpenAI => openai::complete(&api_key, model, &system, &user),
    }
}

/// Deep interpretation result with responses from both providers and a synthesis.
pub struct DeepReport {
    pub claude_response: String,
    pub gpt_response: String,
    pub synthesis: String,
}

/// Run the deep interpretation pipeline:
///   1. Get Claude's and GPT's interpretations in parallel
///   2. Feed both + raw data to Claude for synthesis and fact-checking
pub fn deep_interpret(
    current: &SessionData,
    history: &[SessionData],
    tier: provider::ModelTier,
) -> Result<DeepReport> {
    let claude = Provider::Anthropic;
    let gpt = Provider::OpenAI;

    // Verify both API keys upfront before making any calls
    let claude_key = claude.api_key()?;
    let gpt_key = gpt.api_key()?;

    let system = prompt::system_prompt();
    let user = prompt::user_prompt(current, history);

    let claude_model = claude.model_for_tier(tier);
    let gpt_model = gpt.model_for_tier(tier);

    // Single runtime for all async work
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create async runtime")?;

    // Step 1: Claude and GPT in parallel
    let (claude_response, gpt_response) = rt.block_on(async {
        tokio::try_join!(
            anthropic::complete_async(&claude_key, claude_model, &system, &user),
            openai::complete_async(&gpt_key, gpt_model, &system, &user),
        )
    })?;

    // Step 2: Synthesis — Claude reviews both interpretations against the raw data
    let synthesis_system = prompt::synthesis_system_prompt();
    let synthesis_user = prompt::synthesis_user_prompt(
        current,
        history,
        &claude_response,
        &gpt_response,
    );

    let synthesis = rt.block_on(
        anthropic::complete_async(&claude_key, claude_model, &synthesis_system, &synthesis_user),
    )?;

    Ok(DeepReport {
        claude_response,
        gpt_response,
        synthesis,
    })
}
