//! OpenAI-compatible text-to-speech adapter.

use std::borrow::Cow;
use std::num::NonZeroUsize;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection};
use crate::routes::errors::ExtractionError;
use crate::speech::core::{
    self as speech, AudioFormat, RenderedAudio, SpeechError, SpeechRequest, SpeechSegment,
};
use crate::types::errors::EncodingError;
use crate::types::http::{ByteStreamResponse, SseEvents, SseResponse};
use crate::types::model::ModelId;

// -----------------------------------------------------------------------------
// OpenaiInputLimit: Caps work to the provider's documented request size.
// -----------------------------------------------------------------------------

/// Maximum input characters accepted by the `OpenAI` compatibility route.
const OPENAI_INPUT_LIMIT: usize = 4_096;

// -----------------------------------------------------------------------------
// AudioChunkSize: Bounds each streamed OpenAI audio event.
// -----------------------------------------------------------------------------

/// Maximum raw audio bytes carried by one streaming delta.
const AUDIO_CHUNK_SIZE: usize = 12 * 1024;

// -----------------------------------------------------------------------------
// SpeechVoice: Accepts both OpenAI voice wire spellings.
// -----------------------------------------------------------------------------

/// Named or object-wrapped voice supplied by an `OpenAI` client.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SpeechVoice {
    /// Direct voice name.
    Name(
        /// Provider voice identifier.
        String,
    ),
    /// Object form used by clients that pass a voice reference.
    Reference {
        /// Provider voice identifier.
        id: String,
    },
}

impl JsonSchema for SpeechVoice {
    fn schema_name() -> Cow<'static, str> {
        "SpeechVoice".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "oneOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"],
                    "additionalProperties": false
                }
            ]
        })
    }
}

impl SpeechVoice {
    /// Extract a nonempty provider voice name.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError::EmptyVoice`] when the supplied name is blank.
    fn into_name(self) -> Result<String, OpenAiError> {
        let voice = match self {
            Self::Name(voice) | Self::Reference { id: voice } => voice,
        };
        (!voice.trim().is_empty())
            .then_some(voice)
            .ok_or(OpenAiError::EmptyVoice)
    }
}

// -----------------------------------------------------------------------------
// SpeechResponseFormat: Maps OpenAI codec names into engine encodings.
// -----------------------------------------------------------------------------

/// `OpenAI` speech response format accepted on the wire.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SpeechResponseFormat {
    /// MPEG Layer III audio.
    #[default]
    Mp3,
    /// RIFF/WAVE signed PCM audio.
    Wav,
    /// Headerless signed PCM audio.
    Pcm,
    /// Opus, which this fixture does not encode.
    Opus,
    /// AAC, which this fixture does not encode.
    Aac,
    /// FLAC, which this fixture does not encode.
    Flac,
    /// Future or unknown provider format.
    #[serde(other)]
    Unsupported,
}

impl TryFrom<SpeechResponseFormat> for AudioFormat {
    type Error = OpenAiError;

    fn try_from(value: SpeechResponseFormat) -> Result<Self, Self::Error> {
        match value {
            SpeechResponseFormat::Mp3 => Ok(Self::Mp3),
            SpeechResponseFormat::Wav => Ok(Self::Wav),
            SpeechResponseFormat::Pcm => Ok(Self::Pcm),
            SpeechResponseFormat::Opus
            | SpeechResponseFormat::Aac
            | SpeechResponseFormat::Flac
            | SpeechResponseFormat::Unsupported => Err(OpenAiError::UnsupportedSpeechFormat),
        }
    }
}

// -----------------------------------------------------------------------------
// SpeechStreamFormat: Describes the OpenAI delivery selector.
// -----------------------------------------------------------------------------

/// `OpenAI` speech delivery format accepted on the wire.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SpeechStreamFormat {
    /// Raw encoded audio bytes.
    #[default]
    Audio,
    /// Base64 audio deltas carried by server-sent events.
    Sse,
    /// Future or unknown provider delivery format.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// SpeechDelivery: Represents a validated OpenAI response transport.
// -----------------------------------------------------------------------------

/// Delivery mode supported by the compatibility route.
#[derive(Debug, Clone, Copy)]
enum SpeechDelivery {
    /// Raw encoded audio bytes.
    Audio,
    /// Base64 audio deltas carried by server-sent events.
    Sse,
}

impl TryFrom<SpeechStreamFormat> for SpeechDelivery {
    type Error = OpenAiError;

    fn try_from(format: SpeechStreamFormat) -> Result<Self, Self::Error> {
        match format {
            SpeechStreamFormat::Audio => Ok(Self::Audio),
            SpeechStreamFormat::Sse => Ok(Self::Sse),
            SpeechStreamFormat::Unsupported => Err(OpenAiError::UnsupportedSpeechStreamFormat),
        }
    }
}

// -----------------------------------------------------------------------------
// SpeechPayload: Defines the OpenAI speech request body.
// -----------------------------------------------------------------------------

/// OpenAI-compatible speech request payload.
#[derive(Debug, Deserialize, JsonSchema)]
struct SpeechPayload {
    /// Requested model identifier.
    model: ModelId,
    /// Text to pronounce.
    input: String,
    /// Named voice or voice reference.
    voice: SpeechVoice,
    /// Optional prose interpreted as simple style keywords.
    instructions: Option<String>,
    /// Optional speaking-rate multiplier.
    speed: Option<f32>,
    /// Optional audio encoding.
    response_format: Option<SpeechResponseFormat>,
    /// Optional raw-audio or SSE delivery selector.
    stream_format: Option<SpeechStreamFormat>,
}

// -----------------------------------------------------------------------------
// SpeechStreamEvent: Defines OpenAI's typed SSE records.
// -----------------------------------------------------------------------------

/// One event in an `OpenAI` speech SSE response.
#[derive(Serialize)]
#[serde(tag = "type")]
enum SpeechStreamEvent {
    /// Base64-encoded audio delta.
    #[serde(rename = "speech.audio.delta")]
    Delta {
        /// Base64-encoded audio bytes.
        audio: String,
    },
    /// Terminal event containing usage estimates.
    #[serde(rename = "speech.audio.done")]
    Done {
        /// Token estimates for the completed request.
        usage: Usage,
    },
}

impl SpeechStreamEvent {
    /// Encode this typed record as a named SSE event.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when JSON serialization fails.
    fn into_event(self) -> Result<Event, EncodingError> {
        let name = match self {
            Self::Delta { .. } => "speech.audio.delta",
            Self::Done { .. } => "speech.audio.done",
        };
        Event::default()
            .event(name)
            .json_data(self)
            .map_err(|source| EncodingError::Sse { source })
    }
}

// -----------------------------------------------------------------------------
// Usage: Reports estimated text and audio token counts.
// -----------------------------------------------------------------------------

/// Usage object returned in the terminal `OpenAI` speech event.
#[derive(Serialize)]
struct Usage {
    /// Approximate input text tokens.
    #[serde(rename = "input_tokens")]
    input: usize,
    /// Approximate generated audio tokens.
    #[serde(rename = "output_tokens")]
    output: usize,
    /// Sum of input and output token estimates.
    #[serde(rename = "total_tokens")]
    total: usize,
}

// -----------------------------------------------------------------------------
// LoweredSpeech: Carries a validated OpenAI request into the engine.
// -----------------------------------------------------------------------------

/// Provider-neutral request plus its selected response policy.
struct LoweredSpeech {
    /// Provider-neutral synthesis input.
    request: SpeechRequest,
    /// Requested audio encoding.
    format: AudioFormat,
    /// Validated response delivery mode.
    delivery: SpeechDelivery,
}

impl TryFrom<SpeechPayload> for LoweredSpeech {
    type Error = OpenAiError;

    fn try_from(payload: SpeechPayload) -> Result<Self, Self::Error> {
        (payload.model.as_str() == speech::MODEL_ID)
            .then_some(())
            .ok_or(OpenAiError::SpeechModelRequired)?;
        let format = payload.response_format.unwrap_or_default().try_into()?;
        let delivery = payload.stream_format.unwrap_or_default().try_into()?;
        let voice = payload.voice.into_name()?;
        Ok(Self {
            request: SpeechRequest {
                segments: vec![SpeechSegment {
                    text: payload.input,
                    voice,
                    style: payload.instructions.unwrap_or_default(),
                    speed: payload.speed.unwrap_or(1.0),
                    pause_after_ms: 0,
                }],
                sample_rate: 24_000,
            },
            format,
            delivery,
        })
    }
}

impl LoweredSpeech {
    /// Render the lowered request through the bounded speech service.
    ///
    /// # Errors
    ///
    /// Returns request validation, synthesis, worker, or encoding failures.
    async fn render(self, state: &AppState) -> Result<RenderedSpeech, SpeechError> {
        let input_tokens = self.request.input_tokens();
        let configured_limit = state.config.limits.max_input_chars();
        let input_limit = configured_limit.get().min(OPENAI_INPUT_LIMIT);
        let input_limit = NonZeroUsize::new(input_limit).unwrap_or(configured_limit);
        let audio = state
            .speech
            .render(self.request, self.format, input_limit)
            .await?;
        let output_tokens = audio.output_tokens();
        Ok(RenderedSpeech {
            audio,
            usage: Usage {
                input: input_tokens,
                output: output_tokens,
                total: input_tokens + output_tokens,
            },
            delivery: self.delivery,
            delay_ms: state.config.stream_delay_ms,
        })
    }
}

// -----------------------------------------------------------------------------
// RenderedSpeech: Owns OpenAI audio response rendering.
// -----------------------------------------------------------------------------

/// Completed speech audio with usage and delivery metadata.
struct RenderedSpeech {
    /// Encoded speech payload.
    audio: RenderedAudio,
    /// Estimated input and output usage.
    usage: Usage,
    /// Raw-audio or SSE response contract.
    delivery: SpeechDelivery,
    /// Optional delay between streamed chunks.
    delay_ms: u64,
}

impl IntoResponse for RenderedSpeech {
    fn into_response(self) -> Response {
        match self.delivery {
            SpeechDelivery::Audio => AudioSpeechResponse(self).into_response(),
            SpeechDelivery::Sse => SseSpeechResponse(self).into_response(),
        }
    }
}

// -----------------------------------------------------------------------------
// AudioSpeechResponse: Delivers raw encoded audio bytes.
// -----------------------------------------------------------------------------

/// `OpenAI` raw-audio response with optional chunk pacing.
struct AudioSpeechResponse(
    /// Completed speech output to deliver.
    RenderedSpeech,
);

impl IntoResponse for AudioSpeechResponse {
    fn into_response(self) -> Response {
        ByteStreamResponse::new(self.0.audio.bytes, self.0.audio.media_type, self.0.delay_ms)
            .into_response()
    }
}

// -----------------------------------------------------------------------------
// SseSpeechResponse: Delivers typed base64 audio events.
// -----------------------------------------------------------------------------

/// `OpenAI` server-sent event speech response.
struct SseSpeechResponse(
    /// Completed speech output to encode as events.
    RenderedSpeech,
);

impl IntoResponse for SseSpeechResponse {
    fn into_response(self) -> Response {
        let response = (|| {
            let rendered = self.0;
            let mut events = rendered
                .audio
                .bytes
                .chunks(AUDIO_CHUNK_SIZE)
                .map(|chunk| {
                    SpeechStreamEvent::Delta {
                        audio: base64::engine::general_purpose::STANDARD.encode(chunk),
                    }
                    .into_event()
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| OpenAiRejection::from_error(&error))?;
            events.push(
                SpeechStreamEvent::Done {
                    usage: rendered.usage,
                }
                .into_event()
                .map_err(|error| OpenAiRejection::from_error(&error))?,
            );
            Ok::<SseResponse, OpenAiRejection>(
                SseEvents::from(events).with_delay(rendered.delay_ms),
            )
        })();
        match response {
            Ok(response) => response.into_response(),
            Err(rejection) => rejection.into_response(),
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAiSpeech: Mounts and serves the OpenAI speech endpoint.
// -----------------------------------------------------------------------------

/// `OpenAI` speech endpoint.
pub(super) struct OpenAiSpeech;

impl OpenAiSpeech {
    /// Mount the speech route and its generated API response shapes.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/audio/speech",
            post_with(Self::handle, |operation| {
                operation
                    .summary("OpenAI text to speech")
                    .tag("openai")
                    .response::<200, Bytes>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }

    /// Authenticate, lower, render, and deliver one speech request.
    async fn handle(
        State(state): State<AppState>,
        headers: HeaderMap,
        payload: Result<Json<SpeechPayload>, JsonRejection>,
    ) -> Response {
        // Authentication failures take precedence over request-body details.
        if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
            return OpenAiRejection::from_error(&error).into_response();
        }

        let Json(payload) = match payload {
            Ok(payload) => payload,
            // Malformed JSON follows the shared extraction error contract.
            Err(error) => {
                return OpenAiRejection::from_error(&ExtractionError::from(error)).into_response();
            }
        };

        let lowered = match LoweredSpeech::try_from(payload) {
            Ok(lowered) => lowered,
            // Provider validation errors finish before synthesis work begins.
            Err(error) => return OpenAiRejection::from_error(&error).into_response(),
        };

        let rendered = match lowered.render(&state).await {
            Ok(rendered) => rendered,
            // Engine failures map through the OpenAI rejection contract.
            Err(error) => return OpenAiRejection::from_error(&error).into_response(),
        };
        rendered.into_response()
    }
}
