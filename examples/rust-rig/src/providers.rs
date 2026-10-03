use anyhow::Result;
use rig::providers::{anthropic, gemini, ollama, openai};

pub(super) const CHAT_MODEL: &str = "eliza-1966";
pub(super) const EMBEDDING_MODEL: &str = "fnv-embed";
pub(super) const IMAGE_MODEL: &str = "eliza-retro-image";
pub(super) const SPEECH_MODEL: &str = "flite";

pub(super) struct Providers {
    pub(super) anthropic: anthropic::Client,
    pub(super) gemini: gemini::Client,
    pub(super) gemini_openai: openai::CompletionsClient,
    pub(super) ollama: ollama::Client,
    pub(super) openai: openai::Client,
    pub(super) openai_chat: openai::CompletionsClient,
}

impl Providers {
    /// Build provider clients rooted at the local server.
    ///
    /// # Errors
    ///
    /// Returns an error when a provider client cannot be constructed.
    pub(super) fn connect(base_url: &str) -> Result<Self> {
        // Point every SDK client at its provider-native namespace.
        let openai = openai::Client::builder()
            .api_key("local")
            .base_url(format!("{base_url}/openai/v1"))
            .build()?;
        let gemini_openai = openai::Client::builder()
            .api_key("local")
            .base_url(format!("{base_url}/gemini/v1beta/openai"))
            .build()?
            .completions_api();
        let anthropic = anthropic::Client::builder()
            .api_key("local")
            .base_url(format!("{base_url}/anthropic"))
            .build()?;
        let gemini = gemini::Client::builder()
            .api_key("local")
            .base_url(format!("{base_url}/gemini"))
            .build()?;
        let ollama = ollama::Client::builder()
            .api_key("local")
            .base_url(format!("{base_url}/ollama"))
            .build()?;

        // Retain both OpenAI transports because Rig models them as distinct clients.
        Ok(Self {
            anthropic,
            gemini,
            gemini_openai,
            ollama,
            openai_chat: openai.clone().completions_api(),
            openai,
        })
    }
}
