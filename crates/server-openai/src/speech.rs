//! OpenAI-compatible text-to-speech adapter.

use std::borrow::Cow;
use std::num::NonZeroUsize;
use std::time::Duration;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use base64::Engine as _;
use eliza_http::context::ProviderAuth;
use eliza_http::errors::EncodingError;
use eliza_http::execution::Execution;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_modality_speech as speech;
use futures_util::{Stream, StreamExt, stream};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection, OpenAiSpeechStreamError};
use crate::context::SpeechState;

// -----------------------------------------------------------------------------
// Speech: Parses requests and streams encoded responses.
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

impl TryFrom<SpeechResponseFormat> for speech::core::AudioFormat {
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

/// Usage object returned in the terminal `OpenAI` speech event.
#[derive(Serialize)]
struct SpeechUsage {
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
        usage: SpeechUsage,
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
            .map_err(|source| EncodingError::Sse {
                detail: source.to_string(),
            })
    }
}

/// Maximum input characters accepted by the `OpenAI` compatibility route.
const OPENAI_INPUT_LIMIT: usize = 4_096;

/// Deliver encoded bytes as they arrive from the speech worker.
fn speech_response_audio_chunks(
    audio: speech::service::Stream,
    delay_ms: u64,
) -> impl Stream<Item = Result<Bytes, OpenAiSpeechStreamError>> {
    stream::unfold(audio, |mut audio| async move {
        match audio.items.recv().await? {
            speech::service::StreamItem::Chunk(bytes) => {
                Some((Ok::<_, OpenAiSpeechStreamError>(Bytes::from(bytes)), audio))
            }
            speech::service::StreamItem::Done { .. } => None,
            speech::service::StreamItem::Failed(error) => {
                Some((Err(OpenAiSpeechStreamError::Speech(error)), audio))
            }
        }
    })
    .then(move |item| async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        item
    })
}

/// Deliver base64 audio deltas followed by exact usage metadata.
fn speech_response_sse_events(
    audio: speech::service::Stream,
    input_tokens: usize,
    delay_ms: u64,
) -> impl Stream<Item = Result<Event, OpenAiSpeechStreamError>> {
    let sample_rate = audio.sample_rate;
    stream::unfold(audio, move |mut audio| async move {
        let item = audio.items.recv().await?;
        let event = match item {
            speech::service::StreamItem::Chunk(bytes) => SpeechStreamEvent::Delta {
                audio: base64::engine::general_purpose::STANDARD.encode(bytes),
            }
            .into_event()
            .map_err(OpenAiSpeechStreamError::Encoding),
            speech::service::StreamItem::Done { sample_count } => {
                let output = speech::core::output_tokens(sample_count, sample_rate);
                SpeechStreamEvent::Done {
                    usage: SpeechUsage {
                        input: input_tokens,
                        output,
                        total: input_tokens + output,
                    },
                }
                .into_event()
                .map_err(OpenAiSpeechStreamError::Encoding)
            }
            speech::service::StreamItem::Failed(error) => {
                Err(OpenAiSpeechStreamError::Speech(error))
            }
        };
        Some((event, audio))
    })
    .then(move |event| async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        event
    })
}

/// `OpenAI` speech body selected after request lowering.
enum SpeechResponse {
    /// Raw encoded audio chunks.
    Audio {
        /// Incremental engine output.
        audio: speech::service::Stream,
        /// Optional transport pacing.
        delay_ms: u64,
    },
    /// Base64 audio events and a terminal usage event.
    Sse {
        /// Incremental engine output.
        audio: speech::service::Stream,
        /// Approximate input text tokens.
        input_tokens: usize,
        /// Optional transport pacing.
        delay_ms: u64,
    },
}

impl IntoResponse for SpeechResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Audio { audio, delay_ms } => {
                let media_type = audio.media_type.clone();
                let mut response = Body::from_stream(speech_response_audio_chunks(audio, delay_ms))
                    .into_response();
                if let Ok(value) = HeaderValue::from_str(&media_type) {
                    response.headers_mut().insert(header::CONTENT_TYPE, value);
                }
                response
            }
            Self::Sse {
                audio,
                input_tokens,
                delay_ms,
            } => Sse::new(speech_response_sse_events(audio, input_tokens, delay_ms))
                .keep_alive(KeepAlive::default())
                .into_response(),
        }
    }
}

// -----------------------------------------------------------------------------
// LoweredSpeech: Carries a validated OpenAI request into the engine.
// -----------------------------------------------------------------------------

/// Provider-neutral request plus its selected response policy.
struct LoweredSpeech {
    /// Provider-neutral synthesis input.
    request: speech::core::Request,
    /// Requested audio encoding.
    format: speech::core::AudioFormat,
    /// Validated response delivery mode.
    delivery: SpeechDelivery,
}

impl TryFrom<SpeechPayload> for LoweredSpeech {
    type Error = OpenAiError;

    fn try_from(payload: SpeechPayload) -> Result<Self, Self::Error> {
        (payload.model.as_str() == speech::core::MODEL_ID)
            .then_some(())
            .ok_or(OpenAiError::SpeechModelRequired)?;
        let format = payload.response_format.unwrap_or_default().try_into()?;
        let delivery = payload.stream_format.unwrap_or_default().try_into()?;
        let voice = payload.voice.into_name()?;
        Ok(Self {
            request: speech::core::Request {
                segments: vec![speech::core::Segment {
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
    /// Start synthesis and select the requested transport.
    ///
    /// # Errors
    ///
    /// Returns failures detected before HTTP response delivery begins.
    async fn respond(
        self,
        state: &SpeechState,
        execution: Option<Execution>,
    ) -> Result<SpeechResponse, speech::errors::Error> {
        let input_tokens = self.request.input_tokens();
        let configured_limit = state.config.limits.max_input_chars();
        let input_limit = configured_limit.get().min(OPENAI_INPUT_LIMIT);
        let input_limit = NonZeroUsize::new(input_limit).unwrap_or(configured_limit);
        let audio = state
            .speech
            .stream_guarded(
                self.request,
                self.format,
                input_limit,
                execution.map(|value| value.guard()),
            )
            .await?;
        let delay_ms = state.config.stream_delay_ms;
        Ok(match self.delivery {
            SpeechDelivery::Audio => SpeechResponse::Audio { audio, delay_ms },
            SpeechDelivery::Sse => SpeechResponse::Sse {
                audio,
                input_tokens,
                delay_ms,
            },
        })
    }
}

// -----------------------------------------------------------------------------
// Handle: Executes one speech request.
// -----------------------------------------------------------------------------

/// Authenticate, lower, render, and deliver one speech request.
async fn handle(
    State(state): State<SpeechState>,
    headers: HeaderMap,
    execution: Option<Extension<Execution>>,
    payload: Result<Json<SpeechPayload>, JsonRejection>,
) -> Response {
    // Authentication failures take precedence over request-body details.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
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

    match lowered
        .respond(&state, execution.map(|Extension(value)| value))
        .await
    {
        Ok(response) => response.into_response(),
        // Engine failures map through the OpenAI rejection contract.
        Err(error) => OpenAiRejection::from(&error).into_response(),
    }
}

// -----------------------------------------------------------------------------
// Router: Publishes the speech endpoint.
// -----------------------------------------------------------------------------

/// Build the speech route and its generated API response shapes.
pub(super) fn router() -> ApiRouter<SpeechState> {
    ApiRouter::new().api_route(
        "/v1/audio/speech",
        post_with(handle, |operation| {
            operation
                .summary("OpenAI text to speech")
                .tag("openai")
                .response::<200, Bytes>()
                .response::<429, Json<OpenAiFailureResponse>>()
                .response::<503, Json<OpenAiFailureResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        }),
    )
}
