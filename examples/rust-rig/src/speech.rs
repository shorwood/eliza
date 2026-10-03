use anyhow::Result;
use rig::audio_generation::AudioGenerationModel;
use rig::client::audio_generation::AudioGenerationClient;

use crate::providers::{Providers, SPEECH_MODEL};
use crate::report::report;

pub(super) async fn run(providers: &Providers) -> usize {
    let model = providers.openai.audio_generation_model(SPEECH_MODEL);
    let request = model.audio_generation_request().text("Hello from ELIZA.");
    let result: Result<String> = request
        .voice("retro")
        .send()
        .await
        .map(|response| format!("{} audio bytes", response.audio.len()))
        .map_err(Into::into);
    report("OpenAI speech", result)
}
