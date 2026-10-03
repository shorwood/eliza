use anyhow::{Result, ensure};
use rig::client::CompletionClient;
use rig::completion::{AssistantContent, CompletionModel};
use serde_json::{Value, json};

use crate::providers::{CHAT_MODEL, Providers};
use crate::report::report;

const PROMPT: &str = "I am sad.";

pub(super) async fn run(providers: &Providers) -> usize {
    let openai = providers.openai.completion_model(CHAT_MODEL);
    let anthropic = providers.anthropic.completion_model(CHAT_MODEL);
    let gemini = providers.gemini.completion_model(CHAT_MODEL);
    let ollama = providers.ollama.completion_model(CHAT_MODEL);

    // Keep each provider's native opt-in visible beside its result label.
    report(
        "OpenAI Responses reasoning",
        trace(&openai, 128, json!({ "reasoning": { "summary": "auto" } })).await,
    ) + report(
        "Anthropic reasoning",
        trace(
            &anthropic,
            1152,
            json!({ "thinking": { "type": "enabled", "budget_tokens": 1024 } }),
        )
        .await,
    ) + report(
        "Gemini reasoning",
        trace(
            &gemini,
            128,
            json!({
                "generationConfig": {
                    "thinkingConfig": { "includeThoughts": true }
                }
            }),
        )
        .await,
    ) + report(
        "Ollama reasoning",
        trace(&ollama, 128, json!({ "think": true })).await,
    )
}

/// Request and extract one provider-native reasoning trace.
///
/// # Errors
///
/// Returns a provider error or fails when the trace is absent.
async fn trace<M>(model: &M, max_tokens: u64, options: Value) -> Result<String>
where
    M: CompletionModel + Clone,
{
    let request = model
        .clone()
        .completion_request(PROMPT)
        .max_tokens(max_tokens);
    let response = request.additional_params(options).send().await?;
    let traces = response
        .choice
        .into_iter()
        .filter_map(|content| match content {
            AssistantContent::Reasoning(reasoning) => Some(reasoning.display_text()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let trace = traces.join("\n");
    ensure!(
        !trace.is_empty(),
        "response omitted the requested reasoning trace"
    );
    Ok(trace)
}
