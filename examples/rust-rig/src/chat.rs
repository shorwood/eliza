use anyhow::Result;
use futures_util::StreamExt;
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, CompletionModel};
use rig::streaming::StreamedAssistantContent;

use crate::providers::{CHAT_MODEL, Providers};
use crate::report::report;

const PROMPT: &str = "I am sad.";

pub(super) async fn run(providers: &Providers) -> usize {
    exercise(
        "OpenAI Chat Completions",
        &providers.openai_chat.completion_model(CHAT_MODEL),
    )
    .await
        + exercise(
            "OpenAI Responses",
            &providers.openai.completion_model(CHAT_MODEL),
        )
        .await
        + exercise(
            "Gemini OpenAI-compatible alias",
            &providers.gemini_openai.completion_model(CHAT_MODEL),
        )
        .await
        + exercise(
            "Anthropic Messages",
            &providers.anthropic.completion_model(CHAT_MODEL),
        )
        .await
        + exercise(
            "Gemini generateContent",
            &providers.gemini.completion_model(CHAT_MODEL),
        )
        .await
        + exercise(
            "Ollama chat",
            &providers.ollama.completion_model(CHAT_MODEL),
        )
        .await
}

/// Collect one non-streaming response as text.
///
/// # Errors
///
/// Returns the provider's request or response error.
async fn complete<M>(model: &M) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let request = model.clone().completion_request(PROMPT).max_tokens(128);
    let response = request.send().await?;
    Ok(response
        .choice
        .into_iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text),
            _ => None,
        })
        .collect())
}

async fn exercise<M>(name: &str, model: &M) -> usize
where
    M: CompletionModel + Clone,
{
    report(name, complete(model).await) + report(&format!("{name} stream"), stream(model).await)
}

/// Collect one streaming response as text.
///
/// # Errors
///
/// Returns an error when the stream cannot be opened or consumed.
async fn stream<M>(model: &M) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let request = model.clone().completion_request(PROMPT).max_tokens(128);
    let mut response = request.stream().await?;
    let mut text = String::new();
    while let Some(content) = response.next().await {
        if let StreamedAssistantContent::Text(chunk) = content? {
            text.push_str(&chunk.text);
        }
    }
    Ok(text)
}
