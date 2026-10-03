use anyhow::Result;
use rig::client::image_generation::ImageGenerationClient;
use rig::image_generation::ImageGenerationModel;

use crate::providers::{IMAGE_MODEL, Providers};
use crate::report::report;

const PROMPT: &str = "A neon terminal dreaming in magenta and cyan.";

pub(super) async fn run(providers: &Providers) -> usize {
    let openai = providers.openai.image_generation_model(IMAGE_MODEL);
    let gemini = providers.gemini.image_generation_model(IMAGE_MODEL);

    report("OpenAI image generation", generate(&openai).await)
        + report("Gemini image generation", generate(&gemini).await)
}

/// Generate one square image and report its encoded size.
///
/// # Errors
///
/// Returns the provider's image-generation error.
async fn generate<M>(model: &M) -> Result<String>
where
    M: ImageGenerationModel,
{
    let request = model.image_generation_request().prompt(PROMPT).width(256);
    let response = request.height(256).send().await?;
    Ok(format!("{} PNG bytes", response.image.len()))
}
