use anyhow::Result;
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, CompletionModel};
use rig::message::{ImageMediaType, UserContent};

use crate::providers::{CHAT_MODEL, Providers};
use crate::report::report;

const IMAGE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg==";

pub(super) async fn run(providers: &Providers) -> usize {
    inspect(
        "OpenAI Chat image input",
        &providers.openai_chat.completion_model(CHAT_MODEL),
    )
    .await
        + inspect(
            "OpenAI Responses image input",
            &providers.openai.completion_model(CHAT_MODEL),
        )
        .await
        + inspect(
            "Gemini OpenAI-compatible image input",
            &providers.gemini_openai.completion_model(CHAT_MODEL),
        )
        .await
        + inspect(
            "Anthropic image input",
            &providers.anthropic.completion_model(CHAT_MODEL),
        )
        .await
        + inspect(
            "Gemini image input",
            &providers.gemini.completion_model(CHAT_MODEL),
        )
        .await
        + inspect(
            "Ollama image input",
            &providers.ollama.completion_model(CHAT_MODEL),
        )
        .await
}

/// Send the fixture image and collect the model's text response.
///
/// # Errors
///
/// Returns the provider's request or response error.
async fn describe<M>(model: &M) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let prompt = vec![
        UserContent::text("What can you measure in this image?"),
        UserContent::image_base64(IMAGE, Some(ImageMediaType::PNG), None),
    ];
    let request = model.clone().completion_request(prompt).max_tokens(128);
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

async fn inspect<M>(name: &str, model: &M) -> usize
where
    M: CompletionModel + Clone,
{
    report(name, describe(model).await)
}
