//! Exercise ELIZA through Rig's provider clients.

mod errors;

use anyhow::Result;
use futures_util::StreamExt;
use rig::client::{CompletionClient, ModelListingClient};
use rig::completion::{AssistantContent, CompletionModel};
use rig::providers::{anthropic, gemini, ollama, openai};
use rig::streaming::StreamedAssistantContent;

use crate::errors::RigExampleError;

// -----------------------------------------------------------------------------
// Example: Holds shared input sent through every provider surface.
// -----------------------------------------------------------------------------

/// Provider-visible ELIZA model identifier.
const EXAMPLE_MODEL: &str = "eliza-doctor";

/// Deterministic prompt used to make provider results easy to compare.
const EXAMPLE_PROMPT: &str = "I am sad.";

// -----------------------------------------------------------------------------
// Exercise: Runs one model through both transports.
// -----------------------------------------------------------------------------

/// Exercise one model's unary and streaming transports.
async fn exercise<M>(name: &str, model: &M) -> usize
where
    M: CompletionModel + Clone,
{
    let unary_failed = !is_successful(name, complete(model).await);
    let stream_failed = !is_successful(&format!("{name} stream"), stream(model).await);
    usize::from(unary_failed) + usize::from(stream_failed)
}

// -----------------------------------------------------------------------------
// EnsureSuccess: Converts the aggregate outcome into the example's result.
// -----------------------------------------------------------------------------

/// Accept a completed example run only when every call succeeded.
///
/// # Errors
///
/// Returns [`RigExampleError::Failures`] with the accumulated failure count.
fn ensure_success(failures: usize) -> Result<()> {
    match failures {
        0 => Ok(()),
        count => Err(RigExampleError::Failures { count }.into()),
    }
}

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
        .base_url(format!("{base_url}/openai/v1"))
        .build()?;
    let openai_chat = openai.clone().completions_api();
    let gemini_openai = openai::Client::builder()
        .api_key("local")
        .base_url(format!("{base_url}/gemini/v1beta/openai"))
        .build()?
        .completions_api();
    let anthropic_base = format!("{base_url}/anthropic");
    let anthropic = anthropic::Client::builder()
        .api_key("local")
        .base_url(&anthropic_base)
        .build()?;
    let gemini_base = format!("{base_url}/gemini");
    let gemini = gemini::Client::builder()
        .api_key("local")
        .base_url(&gemini_base)
        .build()?;
    let ollama_base = format!("{base_url}/ollama");
    let ollama = ollama::Client::builder()
        .api_key("local")
        .base_url(&ollama_base)
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
    failures += exercise(
        "OpenAI Chat Completions",
        &openai_chat.completion_model(EXAMPLE_MODEL),
    )
    .await;
    failures += exercise("OpenAI Responses", &openai.completion_model(EXAMPLE_MODEL)).await;
    failures += exercise(
        "Gemini OpenAI-compatible alias",
        &gemini_openai.completion_model(EXAMPLE_MODEL),
    )
    .await;
    failures += exercise(
        "Anthropic Messages",
        &anthropic.completion_model(EXAMPLE_MODEL),
    )
    .await;
    failures += usize::from(!is_successful(
        "Gemini models",
        gemini
            .list_models()
            .await
            .map(|models| format!("{} model(s)", models.len()))
            .map_err(Into::into),
    ));
    failures += exercise(
        "Gemini generateContent",
        &gemini.completion_model(EXAMPLE_MODEL),
    )
    .await;
    failures += usize::from(!is_successful(
        "Ollama models",
        ollama
            .list_models()
            .await
            .map(|models| format!("{} model(s)", models.len()))
            .map_err(Into::into),
    ));
    failures += exercise("Ollama chat", &ollama.completion_model(EXAMPLE_MODEL)).await;

    ensure_success(failures)
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
    let request = model
        .clone()
        .completion_request(EXAMPLE_PROMPT)
        .max_tokens(128);
    let response = request.send().await?;
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
    let request = model
        .clone()
        .completion_request(EXAMPLE_PROMPT)
        .max_tokens(128);
    let mut response = request.stream().await?;
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
