//! Gemini speech-generation lowering and rendering.
use axum::Json;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;

use super::errors::{GeminiError, GeminiRejection};
use super::types::{
    Content, ContentPart, ContentRole, GenerateCandidate, GenerateContentRequest,
    GenerateContentResponse, GenerateFinishReason, GenerateInlineData, GenerateOutputContent,
    GenerateOutputPart, GenerateUsage, SpeechAudioFormat, SpeechConfig, SpeechResponseModality,
    SpeechSpeakerVoiceConfig, SpeechVoiceConfig,
};
use crate::routes::context::AppState;
use crate::speech::core::{
    self as speech, AudioFormat, RenderedAudio, SpeechError, SpeechRequest, SpeechSegment,
};
use crate::types::http::{SseEvents, SseResponse, json_event};
use crate::types::model::ModelId;
use crate::types::turn::TokenUsage;

// -----------------------------------------------------------------------------
// AudioChunkSize: Bounds each Gemini streaming record.
// -----------------------------------------------------------------------------

/// Maximum raw audio bytes carried by one streaming record.
const AUDIO_CHUNK_SIZE: usize = 12 * 1024;

// -----------------------------------------------------------------------------
// MultiSpeakerCount: Fixes the supported dialogue arity.
// -----------------------------------------------------------------------------

/// Number of speakers accepted by Gemini's multi-speaker contract.
const MULTI_SPEAKER_COUNT: usize = 2;

// -----------------------------------------------------------------------------
// RequestMode: Selects text or speech generation from the wire request.
// -----------------------------------------------------------------------------

/// Generation mode selected by `responseModalities`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum RequestMode {
    /// Existing ELIZA text generation.
    Text,
    /// Retro diphone audio generation.
    Audio,
}

impl RequestMode {
    /// Interpret the modality selector without consuming the request.
    ///
    /// # Errors
    ///
    /// Returns an invalid-modality error for unsupported modality lists.
    pub(super) fn for_payload(payload: &GenerateContentRequest) -> Result<Self, GeminiError> {
        let modalities = payload
            .generation_config
            .as_ref()
            .and_then(|config| config.response_modalities.as_deref());
        match modalities {
            None | Some([SpeechResponseModality::Text]) => Ok(Self::Text),
            Some([SpeechResponseModality::Audio]) => Ok(Self::Audio),
            _ => Err(GeminiError::InvalidResponseModalities),
        }
    }
}

// -----------------------------------------------------------------------------
// SpeechDelivery: Names the three Gemini response transports.
// -----------------------------------------------------------------------------

/// Delivery contract selected by the route path and `alt` query.
#[derive(Debug, Clone, Copy)]
pub(super) enum SpeechDelivery {
    /// One complete JSON response.
    Unary,
    /// A JSON array of incremental responses.
    JsonStream,
    /// Incremental responses encoded as server-sent events.
    Sse,
}

impl SpeechDelivery {
    /// Return whether this delivery mode uses chunked response records.
    const fn is_streaming(self) -> bool {
        matches!(self, Self::JsonStream | Self::Sse)
    }
}

// -----------------------------------------------------------------------------
// ConfiguredVoice: Extracts one nonempty provider voice name.
// -----------------------------------------------------------------------------

/// Prefer the direct voice spelling while accepting Gemini's nested spelling.
fn configured_voice(config: SpeechVoiceConfig) -> Option<String> {
    config
        .voice
        .or_else(|| {
            config
                .prebuilt_voice_config
                .and_then(|voice| voice.voice_name)
        })
        .filter(|voice| !voice.trim().is_empty())
}

// -----------------------------------------------------------------------------
// SpeakerMapping: Associates a declared speaker with a voice.
// -----------------------------------------------------------------------------

/// Validated Gemini speaker-to-voice association.
struct SpeakerMapping {
    /// Speaker label referenced by content metadata.
    name: String,
    /// Provider voice used for that speaker.
    voice: String,
}

impl TryFrom<SpeechSpeakerVoiceConfig> for SpeakerMapping {
    type Error = GeminiError;

    fn try_from(config: SpeechSpeakerVoiceConfig) -> Result<Self, Self::Error> {
        let name = config
            .speaker
            .filter(|speaker| !speaker.trim().is_empty())
            .ok_or(GeminiError::InvalidSpeakerConfig)?;
        let voice = config
            .voice_config
            .and_then(configured_voice)
            .ok_or(GeminiError::InvalidSpeakerConfig)?;
        Ok(Self { name, voice })
    }
}

// -----------------------------------------------------------------------------
// SpeakerMappings: Enforces the two-speaker Gemini contract.
// -----------------------------------------------------------------------------

/// Exactly two distinct speaker mappings.
struct SpeakerMappings(
    /// Mappings ordered exactly as supplied by the provider request.
    [SpeakerMapping; MULTI_SPEAKER_COUNT],
);

impl TryFrom<Vec<SpeechSpeakerVoiceConfig>> for SpeakerMappings {
    type Error = GeminiError;

    fn try_from(configs: Vec<SpeechSpeakerVoiceConfig>) -> Result<Self, Self::Error> {
        let mappings = configs
            .into_iter()
            .map(SpeakerMapping::try_from)
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| GeminiError::InvalidSpeakerConfig)?;
        let mappings = Self(mappings);
        if mappings.0[0].name == mappings.0[1].name {
            Err(GeminiError::InvalidSpeakerConfig)
        } else {
            Ok(mappings)
        }
    }
}

impl SpeakerMappings {
    /// Resolve a voice for one content speaker label.
    ///
    /// # Errors
    ///
    /// Returns an unknown-speaker error when no mapping has that label.
    fn voice_for(&self, speaker: &str) -> Result<String, GeminiError> {
        self.0
            .iter()
            .find(|mapping| mapping.name == speaker)
            .map(|mapping| mapping.voice.clone())
            .ok_or(GeminiError::UnknownSpeaker)
    }
}

// -----------------------------------------------------------------------------
// VoiceMode: Resolves content metadata into provider-neutral segments.
// -----------------------------------------------------------------------------

/// Validated single- or multi-speaker configuration.
enum VoiceMode {
    /// Every content part uses one voice and must omit a speaker label.
    Single(
        /// Voice shared by every content part.
        String,
    ),
    /// Every content part selects one of two declared speaker labels.
    Multi(
        /// Validated mappings used to resolve each speaker label.
        SpeakerMappings,
    ),
}

impl VoiceMode {
    /// Consume and validate Gemini's nested voice configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for missing, conflicting, or invalid voice configuration.
    fn from_config(config: &mut SpeechConfig) -> Result<Self, GeminiError> {
        match config.multi_speaker_voice_config.take() {
            Some(_) if config.voice_config.is_some() => Err(GeminiError::InvalidSpeakerConfig),
            Some(multi) => multi
                .speaker_voice_configs
                .ok_or(GeminiError::InvalidSpeakerConfig)?
                .try_into()
                .map(Self::Multi),
            None => config
                .voice_config
                .take()
                .and_then(configured_voice)
                .map(Self::Single)
                .ok_or(GeminiError::MissingSpeechConfig),
        }
    }

    /// Lower one text part while enforcing its speaker metadata policy.
    ///
    /// # Errors
    ///
    /// Returns an error for non-text content or mismatched speaker metadata.
    fn segment(&self, part: ContentPart) -> Result<SpeechSegment, GeminiError> {
        let (text, metadata) = match part {
            ContentPart::Text {
                text,
                speech_metadata,
            } => Ok((text, speech_metadata)),
            _ => Err(GeminiError::SpeechTextOnly),
        }?;
        let speaker = metadata.as_ref().and_then(|value| value.speaker.as_deref());
        let (voice, pause_after_ms) = match (self, speaker) {
            (Self::Single(voice), None) => Ok((voice.clone(), 55)),
            (Self::Multi(mappings), Some(speaker)) => Ok((mappings.voice_for(speaker)?, 150)),
            (Self::Single(_), Some(_)) | (Self::Multi(_), None) => Err(GeminiError::UnknownSpeaker),
        }?;
        Ok(SpeechSegment {
            text,
            voice,
            style: metadata.and_then(|value| value.style).unwrap_or_default(),
            speed: 1.0,
            pause_after_ms,
        })
    }
}

// -----------------------------------------------------------------------------
// SpeechContents: Lowers Gemini content into ordered speech segments.
// -----------------------------------------------------------------------------

/// Gemini content consumed specifically by speech generation.
struct SpeechContents(
    /// Ordered provider content blocks.
    Vec<Content>,
);

impl SpeechContents {
    /// Lower user text parts under a validated voice mode.
    ///
    /// # Errors
    ///
    /// Returns an error when content is empty, non-user, or non-text.
    fn lower(self, voices: &VoiceMode) -> Result<Vec<SpeechSegment>, GeminiError> {
        let mut segments = Vec::new();
        for content in self.0 {
            matches!(content.role, None | Some(ContentRole::User))
                .then_some(())
                .ok_or(GeminiError::SpeechTextOnly)?;
            let parts = content.parts.ok_or(GeminiError::SpeechTextOnly)?;
            (!parts.is_empty())
                .then_some(())
                .ok_or(GeminiError::SpeechTextOnly)?;
            for part in parts {
                segments.push(voices.segment(part)?);
            }
        }
        if let Some(last) = segments.last_mut() {
            last.pause_after_ms = 0;
        }
        Ok(segments)
    }
}

// -----------------------------------------------------------------------------
// AudioFormat: Maps Gemini media choices to engine encodings.
// -----------------------------------------------------------------------------

impl TryFrom<SpeechAudioFormat> for AudioFormat {
    type Error = GeminiError;

    fn try_from(format: SpeechAudioFormat) -> Result<Self, Self::Error> {
        match format {
            SpeechAudioFormat::AudioWav => Ok(Self::Wav),
            SpeechAudioFormat::AudioL16 => Ok(Self::L16),
            SpeechAudioFormat::AudioMulaw => Ok(Self::MuLaw),
            SpeechAudioFormat::AudioAlaw => Ok(Self::ALaw),
            SpeechAudioFormat::Unsupported => Err(GeminiError::UnsupportedSpeechFormat),
        }
    }
}

// -----------------------------------------------------------------------------
// LoweredSpeech: Carries one validated engine request and encoding.
// -----------------------------------------------------------------------------

/// Fully lowered Gemini speech operation.
struct LoweredSpeech {
    /// Provider-neutral synthesis input.
    request: SpeechRequest,
    /// Provider-neutral response encoding.
    format: AudioFormat,
}

impl LoweredSpeech {
    /// Lower one provider request into the speech engine contract.
    ///
    /// # Errors
    ///
    /// Returns a provider error for unsupported controls, encoding, content, or voice settings.
    fn from_payload(
        payload: GenerateContentRequest,
        delivery: SpeechDelivery,
    ) -> Result<Self, GeminiError> {
        let has_unsupported_controls = payload.system_instruction.is_some()
            || payload.tools.is_some()
            || payload.tool_config.is_some();
        (!has_unsupported_controls)
            .then_some(())
            .ok_or(GeminiError::SpeechControlsUnsupported)?;
        let mut generation = payload
            .generation_config
            .ok_or(GeminiError::MissingSpeechConfig)?;
        let audio = generation
            .response_format
            .take()
            .and_then(|format| format.audio);
        let requested_format = audio
            .as_ref()
            .and_then(|format| format.mime_type)
            .unwrap_or(if delivery.is_streaming() {
                SpeechAudioFormat::AudioL16
            } else {
                SpeechAudioFormat::AudioWav
            });
        let sample_rate = audio
            .and_then(|format| format.sample_rate)
            .unwrap_or(24_000);
        let mut config = generation
            .speech_config
            .take()
            .ok_or(GeminiError::MissingSpeechConfig)?;
        let voices = VoiceMode::from_config(&mut config)?;
        let contents = payload.contents.ok_or(GeminiError::SpeechTextOnly)?;
        Ok(Self {
            request: SpeechRequest {
                segments: SpeechContents(contents).lower(&voices)?,
                sample_rate,
            },
            format: requested_format.try_into()?,
        })
    }

    /// Execute the provider-neutral request and retain response metadata.
    ///
    /// # Errors
    ///
    /// Returns request validation, synthesis, worker, or encoding failures.
    async fn render(self, state: &AppState) -> Result<RenderedSpeech, SpeechError> {
        let prompt_tokens = self.request.input_tokens();
        let audio = state
            .speech
            .render(
                self.request,
                self.format,
                state.config.limits.max_input_chars(),
            )
            .await?;
        Ok(RenderedSpeech {
            audio,
            prompt_tokens,
        })
    }
}

// -----------------------------------------------------------------------------
// RenderedSpeech: Retains engine output with Gemini response metadata.
// -----------------------------------------------------------------------------

/// Completed engine result ready for provider rendering.
struct RenderedSpeech {
    /// Encoded engine output.
    audio: RenderedAudio,
    /// Estimated input token count captured before rendering.
    prompt_tokens: usize,
}

// -----------------------------------------------------------------------------
// InlineAudio: Keeps MIME data paired while building Gemini responses.
// -----------------------------------------------------------------------------

/// Base64 audio payload carried by one Gemini part.
struct InlineAudio {
    /// MIME type advertised for the data.
    media_type: String,
    /// Base64-encoded audio bytes.
    data: String,
}

// -----------------------------------------------------------------------------
// SpeechResponse: Renders completed audio through a Gemini transport.
// -----------------------------------------------------------------------------

/// Completed audio plus the provider response metadata needed to deliver it.
struct SpeechResponse {
    /// Unary, JSON-stream, or SSE delivery contract.
    delivery: SpeechDelivery,
    /// Model identifier echoed by Gemini responses.
    model: ModelId,
    /// Encoded speech payload.
    audio: RenderedAudio,
    /// Gemini-specific MIME spelling for the payload.
    media_type: String,
    /// Input and generated-audio token estimates.
    usage: GenerateUsage,
    /// Optional delay between emitted SSE records.
    delay_ms: u64,
}

impl SpeechResponse {
    /// Assemble a completed provider response and derive its wire metadata.
    fn new(
        delivery: SpeechDelivery,
        model: ModelId,
        rendered: RenderedSpeech,
        delay_ms: u64,
    ) -> Self {
        let media_type = rendered.audio.media_type.clone();
        let output_tokens = rendered.audio.output_tokens();
        let usage = TokenUsage {
            prompt: rendered.prompt_tokens,
            completion: output_tokens,
            total: rendered.prompt_tokens + output_tokens,
        }
        .into();
        Self {
            delivery,
            model,
            audio: rendered.audio,
            media_type,
            usage,
            delay_ms,
        }
    }

    /// Build one Gemini response around inline audio.
    fn content(
        model: ModelId,
        audio: InlineAudio,
        finish_reason: Option<GenerateFinishReason>,
        usage_metadata: Option<GenerateUsage>,
    ) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts: vec![GenerateOutputPart::InlineData {
                        inline_data: GenerateInlineData {
                            mime_type: audio.media_type,
                            data: audio.data,
                        },
                    }],
                },
                finish_reason,
                index: 0,
            }],
            model_version: model,
            usage_metadata,
        }
    }

    /// Render one complete JSON response.
    fn into_unary(self) -> Json<GenerateContentResponse> {
        let data = base64::engine::general_purpose::STANDARD.encode(self.audio.bytes);
        Json(Self::content(
            self.model,
            InlineAudio {
                media_type: self.media_type,
                data,
            },
            Some(GenerateFinishReason::Stop),
            Some(self.usage),
        ))
    }

    /// Split encoded audio into Gemini streaming records and a terminal record.
    fn into_records(self) -> Vec<GenerateContentResponse> {
        let mut records = self
            .audio
            .bytes
            .chunks(AUDIO_CHUNK_SIZE)
            .map(|chunk| {
                Self::content(
                    self.model.clone(),
                    InlineAudio {
                        media_type: self.media_type.clone(),
                        data: base64::engine::general_purpose::STANDARD.encode(chunk),
                    },
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();
        records.push(Self::content(
            self.model,
            InlineAudio {
                media_type: self.media_type,
                data: String::new(),
            },
            Some(GenerateFinishReason::Stop),
            Some(self.usage),
        ));
        records
    }

    /// Render streaming records as server-sent events.
    ///
    /// # Errors
    ///
    /// Returns a Gemini rejection when an event cannot be serialized.
    fn into_sse(self) -> Result<SseResponse, GeminiRejection> {
        let delay_ms = self.delay_ms;
        let events = self
            .into_records()
            .iter()
            .map(json_event)
            .collect::<Result<Vec<Event>, _>>();
        events
            .map(|events| SseEvents::from(events).with_delay(delay_ms))
            .map_err(|error| GeminiRejection::from_error(&error))
    }
}

impl IntoResponse for SpeechResponse {
    fn into_response(self) -> Response {
        match self.delivery {
            SpeechDelivery::Unary => self.into_unary().into_response(),
            SpeechDelivery::JsonStream => Json(self.into_records()).into_response(),
            SpeechDelivery::Sse => self.into_sse().into_response(),
        }
    }
}

/// Generate one Gemini unary response or stream record collection.
pub(super) async fn generate(
    state: &AppState,
    model: ModelId,
    delivery: SpeechDelivery,
    payload: GenerateContentRequest,
) -> Response {
    // Only the dedicated speech model can execute AUDIO requests.
    if model.as_str() != speech::MODEL_ID {
        return GeminiRejection::from_error(&GeminiError::SpeechModelRequired).into_response();
    }

    // Lower the provider request before occupying a synthesis worker.
    let lowered = match LoweredSpeech::from_payload(payload, delivery) {
        Ok(lowered) => lowered,
        // Provider lowering errors end the request before synthesis work begins.
        Err(error) => return GeminiRejection::from_error(&error).into_response(),
    };

    // Render validated input through the bounded speech service.
    let rendered = match lowered.render(state).await {
        Ok(rendered) => rendered,
        // Engine failures map directly through the Gemini rejection contract.
        Err(error) => return GeminiRejection::from_error(&error).into_response(),
    };

    SpeechResponse::new(delivery, model, rendered, state.config.stream_delay_ms).into_response()
}
