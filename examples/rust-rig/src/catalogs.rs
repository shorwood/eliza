use anyhow::Result;
use rig::client::ModelListingClient;

use crate::providers::Providers;
use crate::report::report;

pub(super) async fn run(providers: &Providers) -> usize {
    report("OpenAI models", list(&providers.openai).await)
        + report("Gemini models", list(&providers.gemini).await)
        + report("Ollama models", list(&providers.ollama).await)
}

/// Return the number of models exposed by one provider.
///
/// # Errors
///
/// Returns the provider's model-listing error.
async fn list<C>(client: &C) -> Result<String>
where
    C: ModelListingClient,
{
    Ok(format!("{} model(s)", client.list_models().await?.len()))
}
