//! Exercise ELIZA through Rig's provider clients.

use anyhow::Result;
use futures_util::StreamExt;
use rig::client::{CompletionClient, ModelListingClient};
use rig::completion::{AssistantContent, CompletionModel};
use rig::providers::{anthropic, gemini, openai};
use rig::streaming::StreamedAssistantContent;

// -----------------------------------------------------------------------------
// Example: Holds shared input sent through every provider surface.
// -----------------------------------------------------------------------------

/// Provider-visible ELIZA model identifier.
const EXAMPLE_MODEL: &str = "eliza-doctor";

/// Deterministic prompt used to make provider results easy to compare.
const EXAMPLE_PROMPT: &str = "I am sad.";

// -----------------------------------------------------------------------------
// Main: Builds provider clients and runs every supported example.
// -----------------------------------------------------------------------------

/// Run every Rig provider example against ELIZA.
///
/// # Errors
///
/// Returns an error when client construction fails or any example call fails.
///
/// # Panics
///
/// Panics if Tokio cannot initialize the async runtime.
#[tokio::main]
async fn main() -> Result<()> {
    let base_url =
        std::env::var("ELIZA_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".to_owned());

    let openai = openai::Client::builder()
        .api_key("local")
        .base_url(format!("{base_url}/v1"))
        .build()?;
    let openai_chat = openai.clone().completions_api();
    let gemini_openai = openai::Client::builder()
        .api_key("local")
        .base_url(format!("{base_url}/v1beta/openai"))
        .build()?
        .completions_api();
    let anthropic = anthropic::Client::builder()
        .api_key("local")
        .base_url(&base_url)
        .build()?;
    let gemini = gemini::Client::builder()
        .api_key("local")
        .base_url(&base_url)
        .build()?;

    let mut failures = 0;
    failures += usize::from(!is_successful(
        "OpenAI models",
        openai
            .list_models()
            .await
            .map(|models| format!("{} model(s)", models.len()))
            .map_err(Into::into),
    ));
    failures += usize::from(!is_successful(
        "OpenAI Chat Completions",
        complete(&openai_chat.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "OpenAI Chat Completions stream",
        stream(&openai_chat.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "OpenAI Responses",
        complete(&openai.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Gemini OpenAI-compatible alias",
        complete(&gemini_openai.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Gemini OpenAI-compatible alias stream",
        stream(&gemini_openai.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Anthropic Messages",
        complete(&anthropic.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Anthropic Messages stream",
        stream(&anthropic.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Gemini models",
        gemini
            .list_models()
            .await
            .map(|models| format!("{} model(s)", models.len()))
            .map_err(Into::into),
    ));
    failures += usize::from(!is_successful(
        "Gemini generateContent",
        complete(&gemini.completion_model(EXAMPLE_MODEL)).await,
    ));
    failures += usize::from(!is_successful(
        "Gemini streamGenerateContent",
        stream(&gemini.completion_model(EXAMPLE_MODEL)).await,
    ));

    if failures > 0 {
        anyhow::bail!("{failures} Rig example(s) failed");
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Complete: Normalizes unary responses to text.
// -----------------------------------------------------------------------------

/// Send one non-streaming completion and collect its text.
///
/// # Errors
///
/// Returns the provider error when the request or response is incompatible.
async fn complete<M>(model: &M) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let response = model
        .clone()
        .completion_request(EXAMPLE_PROMPT)
        .max_tokens(128)
        .send()
        .await?;
    Ok(response
        .choice
        .into_iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text),
            _ => None,
        })
        .collect::<String>())
}

// -----------------------------------------------------------------------------
// Stream: Normalizes streaming responses to text.
// -----------------------------------------------------------------------------

/// Send one streaming completion and collect its text chunks.
///
/// # Errors
///
/// Returns the provider error when the stream cannot be opened or consumed.
async fn stream<M>(model: &M) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let mut response = model
        .clone()
        .completion_request(EXAMPLE_PROMPT)
        .max_tokens(128)
        .stream()
        .await?;
    let mut text = String::new();
    while let Some(content) = response.next().await {
        if let StreamedAssistantContent::Text(chunk) = content? {
            text.push_str(&chunk.text);
        }
    }
    Ok(text)
}

// -----------------------------------------------------------------------------
// IsSuccessful: Renders one result and tells the caller whether it succeeded.
// -----------------------------------------------------------------------------

/// Print one labeled example result and return whether it succeeded.
fn is_successful(name: &str, result: Result<String>) -> bool {
    match result {
        Ok(output) => {
            println!("OK   {name}: {output}");
            true
        }
        Err(error) => {
            eprintln!("FAIL {name}: {error:#}");
            false
        }
    }
}
