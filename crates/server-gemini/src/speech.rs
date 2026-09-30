//! Gemini speech-generation lowering and rendering.
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, header};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use eliza_http::context::AppState;
use eliza_http::model::ModelId;
use eliza_http::response::json_event;
use eliza_modality_chat::turn::TokenUsage;
use eliza_modality_speech::core::{
    self as speech, AudioFormat, RenderedAudio, SpeechRequest, SpeechSegment,
};
use eliza_modality_speech::errors::SpeechError;
use eliza_modality_speech::service::{SpeechStream, SpeechStreamItem};
use futures_util::{Stream, StreamExt, stream};

use super::errors::{GeminiError, GeminiRejection, GeminiSpeechStreamError};
use super::types::{
    Content, ContentPart, ContentRole, GenerateCandidate, GenerateContentRequest,
    GenerateContentResponse, GenerateFinishReason, GenerateInlineData, GenerateOutputContent,
    GenerateOutputPart, GenerateUsage, SpeechAudioFormat, SpeechConfig, SpeechResponseModality,
    SpeechSpeakerVoiceConfig, SpeechVoiceConfig,
};

// -----------------------------------------------------------------------------
// Speech: Selects and validates Gemini speech generation.
// -----------------------------------------------------------------------------

/// Generation mode selected by `responseModalities`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum SpeechRequestMode {
    /// Existing ELIZA text generation.
    Text,
    /// Retro diphone audio generation.
    Audio,
}

impl SpeechRequestMode {
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
            Some([SpeechResponseModality::Audio])
                if payload
                    .generation_config
                    .as_ref()
                    .is_some_and(super::types::GenerateConfig::has_text_format_controls) =>
            {
                Err(GeminiError::TextFormatForAudio)
            }
            Some([SpeechResponseModality::Audio]) => Ok(Self::Audio),
            _ => Err(GeminiError::InvalidResponseModalities),
        }
    }
}

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

/// Number of speakers accepted by Gemini's multi-speaker contract.
const MULTI_SPEAKER_COUNT: usize = 2;

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

/// Validated Gemini speaker-to-voice association.
struct SpeechSpeaker {
    /// Speaker label referenced by content metadata.
    name: String,
    /// Provider voice used for that speaker.
    voice: String,
}

impl TryFrom<SpeechSpeakerVoiceConfig> for SpeechSpeaker {
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

/// Exactly two distinct speaker mappings.
struct SpeechSpeakers(
    /// Mappings ordered exactly as supplied by the provider request.
    [SpeechSpeaker; MULTI_SPEAKER_COUNT],
);

impl TryFrom<Vec<SpeechSpeakerVoiceConfig>> for SpeechSpeakers {
    type Error = GeminiError;

    fn try_from(configs: Vec<SpeechSpeakerVoiceConfig>) -> Result<Self, Self::Error> {
        let mappings = configs
            .into_iter()
            .map(SpeechSpeaker::try_from)
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

impl SpeechSpeakers {
    /// Resolve a voice for one content speaker label.
    ///
    /// # Errors
    ///
    /// Returns an unknown-speaker error when no mapping has that label.
    fn voice_for(&self, speaker: &str) -> Result<String, GeminiError> {
        let mapping = self
            .0
            .iter()
            .find(|mapping| mapping.name == speaker)
            .ok_or(GeminiError::UnknownSpeaker)?;
        Ok(mapping.voice.clone())
    }
}

/// Validated single- or multi-speaker configuration.
enum SpeechVoice {
    /// Every content part uses one voice and must omit a speaker label.
    Single(
        /// Voice shared by every content part.
        String,
    ),
    /// Every content part selects one of two declared speaker labels.
    Multi(
        /// Validated mappings used to resolve each speaker label.
        SpeechSpeakers,
    ),
}

impl SpeechVoice {
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
            None => {
                let voice = config.voice_config.take().and_then(configured_voice);
                voice
                    .map(Self::Single)
                    .ok_or(GeminiError::MissingSpeechConfig)
            }
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
    fn lower(self, voices: &SpeechVoice) -> Result<Vec<SpeechSegment>, GeminiError> {
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
// SpeechResponse: Owns incremental Gemini response rendering.
// -----------------------------------------------------------------------------

/// Token and sample-rate context needed by the terminal stream record.
#[derive(Clone, Copy)]
struct SpeechResponseUsage {
    /// Approximate input text tokens.
    prompt_tokens: usize,
    /// Samples per second in the uncompressed signal.
    sample_rate: u32,
}

/// Build one Gemini response around inline audio.
fn speech_response_content(
    model: ModelId,
    inline_data: GenerateInlineData,
    finish_reason: Option<GenerateFinishReason>,
    usage_metadata: Option<GenerateUsage>,
) -> GenerateContentResponse {
    GenerateContentResponse {
        candidates: vec![GenerateCandidate {
            content: GenerateOutputContent {
                role: "model",
                parts: vec![GenerateOutputPart::InlineData { inline_data }],
            },
            finish_reason,
            index: 0,
        }],
        model_version: model,
        usage_metadata,
    }
}

/// Render one complete Gemini JSON response.
fn speech_response_unary(
    model: ModelId,
    audio: RenderedAudio,
    prompt_tokens: usize,
) -> Json<GenerateContentResponse> {
    let completion = audio.output_tokens();
    let usage = TokenUsage {
        prompt: prompt_tokens,
        completion,
        total: prompt_tokens + completion,
    }
    .into();
    let data = base64::engine::general_purpose::STANDARD.encode(audio.bytes);
    Json(speech_response_content(
        model,
        GenerateInlineData {
            mime_type: audio.media_type,
            data,
        },
        Some(GenerateFinishReason::Stop),
        Some(usage),
    ))
}

/// Turn one engine stream item into a Gemini response record.
///
/// # Errors
///
/// Returns a typed stream failure when synthesis or encoding fails mid-stream.
fn speech_response_record(
    item: SpeechStreamItem,
    model: ModelId,
    media_type: String,
    usage: SpeechResponseUsage,
) -> Result<GenerateContentResponse, GeminiSpeechStreamError> {
    let (bytes, sample_count) = match item {
        SpeechStreamItem::Chunk(bytes) => (bytes, None),
        SpeechStreamItem::Done { sample_count } => (Vec::new(), Some(sample_count)),
        // A failed worker cannot produce a valid provider record.
        SpeechStreamItem::Failed(error) => {
            return Err(GeminiSpeechStreamError::Speech(error));
        }
    };
    let completion = sample_count.map(|count| speech::output_tokens(count, usage.sample_rate));
    let finish_reason = completion.map(|_| GenerateFinishReason::Stop);
    let usage_metadata = completion.map(|completion| {
        TokenUsage {
            prompt: usage.prompt_tokens,
            completion,
            total: usage.prompt_tokens + completion,
        }
        .into()
    });
    Ok(speech_response_content(
        model,
        GenerateInlineData {
            mime_type: media_type,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        finish_reason,
        usage_metadata,
    ))
}

/// Stream a valid JSON array without retaining its audio records.
fn speech_response_json_body(
    model: ModelId,
    audio: SpeechStream,
    prompt_tokens: usize,
    delay_ms: u64,
) -> Body {
    let records = stream::unfold(Some((audio, true)), move |state| {
        let model = model.clone();
        async move {
            let (mut audio, first) = state?;
            let item = audio.items.recv().await?;
            let terminal = matches!(item, SpeechStreamItem::Done { .. });
            let media_type = audio.media_type.clone();
            let usage = SpeechResponseUsage {
                prompt_tokens,
                sample_rate: audio.sample_rate,
            };
            let encoded =
                speech_response_record(item, model, media_type, usage).and_then(|record| {
                    let mut bytes = serde_json::to_vec(&record)
                        .map_err(|source| GeminiSpeechStreamError::Json { source })?;
                    bytes.insert(0, if first { b'[' } else { b',' });
                    if terminal {
                        bytes.push(b']');
                    }
                    Ok(Bytes::from(bytes))
                });
            let next = (!terminal && encoded.is_ok()).then_some((audio, false));
            Some((encoded, next))
        }
    })
    .then(move |record| async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        record
    });
    Body::from_stream(records)
}

/// Stream Gemini records as server-sent events.
fn speech_response_sse_events(
    model: ModelId,
    audio: SpeechStream,
    prompt_tokens: usize,
    delay_ms: u64,
) -> impl Stream<Item = Result<axum::response::sse::Event, GeminiSpeechStreamError>> {
    stream::unfold(Some(audio), move |state| {
        let model = model.clone();
        async move {
            let mut audio = state?;
            let item = audio.items.recv().await?;
            let terminal = matches!(item, SpeechStreamItem::Done { .. });
            let media_type = audio.media_type.clone();
            let usage = SpeechResponseUsage {
                prompt_tokens,
                sample_rate: audio.sample_rate,
            };
            let event = speech_response_record(item, model, media_type, usage)
                .and_then(|record| json_event(&record).map_err(GeminiSpeechStreamError::Encoding));
            let next = (!terminal && event.is_ok()).then_some(audio);
            Some((event, next))
        }
    })
    .then(move |event| async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        event
    })
}

/// Gemini speech body selected after request lowering.
enum SpeechResponse {
    /// One complete inline-audio response.
    Unary {
        /// Model identifier echoed to the client.
        model: ModelId,
        /// Complete encoded audio.
        audio: RenderedAudio,
        /// Approximate input text tokens.
        prompt_tokens: usize,
    },
    /// Incremental responses serialized as a JSON array.
    JsonStream {
        /// Model identifier echoed in every record.
        model: ModelId,
        /// Incremental encoded audio.
        audio: SpeechStream,
        /// Approximate input text tokens.
        prompt_tokens: usize,
        /// Optional transport pacing.
        delay_ms: u64,
    },
    /// Incremental responses serialized as SSE.
    Sse {
        /// Model identifier echoed in every record.
        model: ModelId,
        /// Incremental encoded audio.
        audio: SpeechStream,
        /// Approximate input text tokens.
        prompt_tokens: usize,
        /// Optional transport pacing.
        delay_ms: u64,
    },
}

impl IntoResponse for SpeechResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Unary {
                model,
                audio,
                prompt_tokens,
            } => speech_response_unary(model, audio, prompt_tokens).into_response(),
            Self::JsonStream {
                model,
                audio,
                prompt_tokens,
                delay_ms,
            } => {
                let mut response = speech_response_json_body(model, audio, prompt_tokens, delay_ms)
                    .into_response();
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                response
            }
            Self::Sse {
                model,
                audio,
                prompt_tokens,
                delay_ms,
            } => {
                let events = speech_response_sse_events(model, audio, prompt_tokens, delay_ms);
                Sse::new(events)
                    .keep_alive(KeepAlive::default())
                    .into_response()
            }
        }
    }
}

// -----------------------------------------------------------------------------
// LoweredSpeech: Carries one validated Gemini request into the engine.
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
        let voices = SpeechVoice::from_config(&mut config)?;
        let contents = payload.contents.ok_or(GeminiError::SpeechTextOnly)?;
        Ok(Self {
            request: SpeechRequest {
                segments: SpeechContents(contents).lower(&voices)?,
                sample_rate,
            },
            format: requested_format.try_into()?,
        })
    }

    /// Render one complete response before returning it.
    ///
    /// # Errors
    ///
    /// Returns a worker, synthesis, or encoding failure.
    async fn respond_unary(
        self,
        state: &AppState,
        model: ModelId,
        prompt_tokens: usize,
    ) -> Result<SpeechResponse, SpeechError> {
        let audio = state
            .speech
            .render(
                self.request,
                self.format,
                state.config.limits.max_input_chars(),
            )
            .await?;
        Ok(SpeechResponse::Unary {
            model,
            audio,
            prompt_tokens,
        })
    }

    /// Start an incremental response in the selected streaming transport.
    ///
    /// # Errors
    ///
    /// Returns a validation or worker-pool failure before delivery begins.
    async fn respond_streaming(
        self,
        state: &AppState,
        model: ModelId,
        delivery: SpeechDelivery,
        prompt_tokens: usize,
    ) -> Result<SpeechResponse, SpeechError> {
        let audio = state
            .speech
            .stream(
                self.request,
                self.format,
                state.config.limits.max_input_chars(),
            )
            .await?;
        let delay_ms = state.config.stream_delay_ms;
        Ok(match delivery {
            SpeechDelivery::JsonStream => SpeechResponse::JsonStream {
                model,
                audio,
                prompt_tokens,
                delay_ms,
            },
            SpeechDelivery::Sse => SpeechResponse::Sse {
                model,
                audio,
                prompt_tokens,
                delay_ms,
            },
            SpeechDelivery::Unary => unreachable!("unary handled before streaming"),
        })
    }

    /// Execute the request through its unary or incremental transport.
    ///
    /// # Errors
    ///
    /// Returns failures detected before HTTP response delivery begins.
    async fn respond(
        self,
        state: &AppState,
        model: ModelId,
        delivery: SpeechDelivery,
    ) -> Result<SpeechResponse, SpeechError> {
        let prompt_tokens = self.request.input_tokens();
        match delivery {
            SpeechDelivery::Unary => self.respond_unary(state, model, prompt_tokens).await,
            SpeechDelivery::JsonStream | SpeechDelivery::Sse => {
                self.respond_streaming(state, model, delivery, prompt_tokens)
                    .await
            }
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
    match lowered.respond(state, model, delivery).await {
        Ok(response) => response.into_response(),
        // Engine failures map directly through the Gemini rejection contract.
        Err(error) => GeminiRejection::from_error(&error).into_response(),
    }
}
