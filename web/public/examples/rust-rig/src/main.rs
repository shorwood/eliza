//! Verify an existing ELIZA instance with Rig's `OpenAI` completion and stream APIs.

use std::time::Duration;

use anyhow::{Result, ensure};
use futures_util::StreamExt;
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, CompletionModel};
use rig::providers::openai;
use rig::streaming::StreamedAssistantContent;

// -----------------------------------------------------------------------------
// Request: Keep the example small and bounded.
// -----------------------------------------------------------------------------

/// Maximum completion length requested from the fixture.
const REQUEST_MAX_OUTPUT_TOKENS: u64 = 128;

/// Deterministic input shared by completion and streaming requests.
const REQUEST_PROMPT: &str = "I am sad.";

/// Overall deadline covering both SDK requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// -----------------------------------------------------------------------------
// Exercise: Verify both SDK transports return the same text.
// -----------------------------------------------------------------------------

/// Compare a completion with its streamed equivalent.
///
/// # Errors
///
/// Returns an error for missing configuration, client/network failures, or
/// empty or inconsistent responses.
async fn exercise() -> Result<()> {
    let root = std::env::var("ELIZA_API_ROOT")?;
    let key = std::env::var("ELIZA_API_KEY").unwrap_or_else(|_| "local".to_owned());
    let builder = openai::Client::builder().api_key(key);
    let builder = builder.base_url(format!("{}/openai/v1", root.trim_end_matches('/')));
    let client = builder.build()?.completions_api();
    let model = client.completion_model("eliza-1966");
    let request = model
        .clone()
        .completion_request(REQUEST_PROMPT)
        .max_tokens(REQUEST_MAX_OUTPUT_TOKENS);
    let response = request.send().await?;
    let text: String = response
        .choice
        .into_iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text),
            _ => None,
        })
        .collect();
    ensure!(!text.is_empty(), "empty completion");
    let mut response = model
        .completion_request(REQUEST_PROMPT)
        .max_tokens(REQUEST_MAX_OUTPUT_TOKENS)
        .stream()
        .await?;
    let mut streamed = String::new();
    while let Some(content) = response.next().await {
        if let StreamedAssistantContent::Text(chunk) = content? {
            streamed.push_str(&chunk.text);
        }
    }
    ensure!(streamed == text, "stream differs from completion");
    println!("{text}");
    Ok(())
}

// -----------------------------------------------------------------------------
// Main: Run the requests with a timer-enabled runtime.
// -----------------------------------------------------------------------------

/// Run the SDK example with an overall deadline.
///
/// # Errors
///
/// Returns the request error or an elapsed deadline.
fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let pending = tokio::time::timeout(REQUEST_TIMEOUT, exercise());
        pending.await?
    })
}
