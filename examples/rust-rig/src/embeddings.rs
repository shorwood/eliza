use anyhow::Result;
use rig::client::EmbeddingsClient;
use rig::embeddings::EmbeddingModel;

use crate::providers::{EMBEDDING_MODEL, Providers};
use crate::report::report;

pub(super) async fn run(providers: &Providers) -> usize {
    report(
        "OpenAI embeddings",
        embed(
            &providers
                .openai
                .embedding_model_with_ndims(EMBEDDING_MODEL, 256),
        )
        .await,
    ) + report(
        "Gemini OpenAI-compatible embeddings",
        embed(
            &providers
                .gemini_openai
                .embedding_model_with_ndims(EMBEDDING_MODEL, 256),
        )
        .await,
    ) + report(
        "Gemini embeddings",
        embed(
            &providers
                .gemini
                .embedding_model_with_ndims(EMBEDDING_MODEL, 256),
        )
        .await,
    ) + report(
        "Ollama embeddings",
        embed(
            &providers
                .ollama
                .embedding_model_with_ndims(EMBEDDING_MODEL, 256),
        )
        .await,
    )
}

/// Embed two related sentences and summarize the returned vectors.
///
/// # Errors
///
/// Returns the provider's embedding error.
async fn embed<M>(model: &M) -> Result<String>
where
    M: EmbeddingModel,
{
    let embeddings = model
        .embed_texts([
            "People sometimes feel unhappy.".to_owned(),
            "People can feel sad.".to_owned(),
        ])
        .await?;
    let dimensions = embeddings
        .first()
        .map_or(0, |embedding| embedding.vec.len());
    Ok(format!(
        "{} vector(s), {dimensions} dimensions",
        embeddings.len()
    ))
}
