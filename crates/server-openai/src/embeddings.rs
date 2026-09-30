//! OpenAI-compatible deterministic embeddings adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::{ProviderAuth, provider_authenticate};
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_modality_embedding as embedding;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// OpenAiEmbedding: Adapts OpenAI's embedding contract.
// -----------------------------------------------------------------------------

/// `OpenAI` embedding output encoding.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum OpenAiEmbeddingEncoding {
    /// JSON floating-point values.
    #[default]
    Float,
    /// Base64-packed values, deliberately outside the initial fixture.
    Base64,
    /// Future or unknown `OpenAI` encoding.
    #[serde(other)]
    Unsupported,
}

/// `OpenAI` input variants retained for explicit subset errors.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum OpenAiEmbeddingInput {
    /// One text value.
    Text(
        /// Text to embed.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Ordered text batch.
    TextBatch(
        /// Text values to embed.
        #[schemars(title = "", description = "")]
        Vec<String>,
    ),
    /// One pre-tokenized value, unsupported by this fixture.
    TokenIds(
        /// Provider token identifiers.
        #[schemars(title = "", description = "")]
        Vec<i64>,
    ),
    /// Batch of pre-tokenized values, unsupported by this fixture.
    TokenIdBatch(
        /// Batches of provider token identifiers.
        #[schemars(title = "", description = "")]
        Vec<Vec<i64>>,
    ),
    /// Any remaining JSON input shape.
    Unsupported(
        /// Unrecognized input retained only for classification.
        #[schemars(title = "", description = "")]
        serde_json::Value,
    ),
}

impl OpenAiEmbeddingInput {
    /// Lower supported text shapes while rejecting token inputs explicitly.
    ///
    /// # Errors
    ///
    /// Returns an error for token identifiers or any other unsupported shape.
    fn into_texts(self) -> Result<Vec<String>, OpenAiError> {
        match self {
            Self::Text(text) => Ok(vec![text]),
            Self::TextBatch(texts) => Ok(texts),
            Self::TokenIds(values) => {
                drop(values);
                Err(OpenAiError::EmbeddingTokenInputUnsupported)
            }
            Self::TokenIdBatch(values) => {
                drop(values);
                Err(OpenAiError::EmbeddingTokenInputUnsupported)
            }
            Self::Unsupported(value) => {
                drop(value);
                Err(OpenAiError::UnsupportedEmbeddingInput)
            }
        }
    }
}

/// OpenAI-compatible embeddings request.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct OpenAiEmbeddingRequest {
    /// Fixed local embedding model.
    model: ModelId,
    /// Text or ordered text batch.
    input: OpenAiEmbeddingInput,
    /// Optional vector size.
    dimensions: Option<i64>,
    /// Optional response encoding.
    encoding_format: Option<OpenAiEmbeddingEncoding>,
    /// End-user metadata accepted as a documented no-op.
    user: Option<String>,
}

/// One indexed vector in an `OpenAI` response.
#[derive(Debug, Serialize, JsonSchema)]
struct OpenAiEmbeddingData {
    /// Wire object discriminator.
    object: &'static str,
    /// Embedding vector.
    embedding: Vec<f32>,
    /// Zero-based input position.
    index: usize,
}

/// `OpenAI` embedding token accounting.
#[derive(Debug, Serialize, JsonSchema)]
struct OpenAiEmbeddingUsage {
    /// Approximate tokens consumed by all inputs.
    prompt_tokens: usize,
    /// Same as prompt tokens because embeddings emit no tokens.
    total_tokens: usize,
}

/// Complete `OpenAI` embedding response.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct OpenAiEmbeddingResponse {
    /// Wire object discriminator.
    object: &'static str,
    /// Ordered indexed vectors.
    data: Vec<OpenAiEmbeddingData>,
    /// Fixed embedding model.
    model: ModelId,
    /// Approximate input usage.
    usage: OpenAiEmbeddingUsage,
}

impl OpenAiEmbeddingResponse {
    /// Restore the provider model around one provider-neutral result.
    fn from_embedding(model: ModelId, response: embedding::engine::Response) -> Self {
        let indexed_embeddings = response.embeddings.into_iter().enumerate();
        let data = indexed_embeddings
            .map(|(index, embedding)| OpenAiEmbeddingData {
                object: "embedding",
                embedding,
                index,
            })
            .collect();
        Self {
            object: "list",
            data,
            model,
            usage: OpenAiEmbeddingUsage {
                prompt_tokens: response.prompt_tokens,
                total_tokens: response.prompt_tokens,
            },
        }
    }
}

/// Authenticate, validate, embed, and render one request.
pub(crate) async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<OpenAiEmbeddingRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use OpenAI's native error envelope.
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return OpenAiRejection::from_error(&error).into_response();
    }

    // Decode the authenticated request body.
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Malformed JSON retains OpenAI's extraction error details.
        Err(error) => {
            return OpenAiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Separate provider controls before lowering input.
    let OpenAiEmbeddingRequest {
        model,
        input,
        dimensions,
        encoding_format,
        user,
    } = payload;

    // User metadata is accepted without affecting deterministic output.
    drop(user);

    // Model routing is a provider concern, not an engine concern.
    if model.as_str() != embedding::engine::MODEL_ID {
        return OpenAiRejection::from_error(&OpenAiError::EmbeddingModelRequired).into_response();
    }

    // This fixture emits JSON floating-point vectors only.
    if !matches!(
        encoding_format.unwrap_or_default(),
        OpenAiEmbeddingEncoding::Float
    ) {
        return OpenAiRejection::from_error(&OpenAiError::UnsupportedEmbeddingEncoding)
            .into_response();
    }

    // Lower supported text input shapes into an ordered batch.
    let texts = match input.into_texts() {
        Ok(texts) => texts,
        // Unsupported input shapes retain OpenAI's error schema.
        Err(error) => {
            return OpenAiRejection::from_error(&error).into_response();
        }
    };

    // Build one provider-neutral batch with a shared vector size.
    let dimensions = dimensions.unwrap_or(embedding::engine::MODEL_DEFAULT_DIMENSIONS);
    let inputs = texts
        .into_iter()
        .map(|text| embedding::engine::Input { text, dimensions })
        .collect();

    // Assemble the provider-neutral embedding batch.
    let request = embedding::engine::Request { inputs };

    // Execute the shared engine and restore OpenAI's envelope.
    match request.complete(state.config.limits.max_input_chars()) {
        Ok(response) => {
            Json(OpenAiEmbeddingResponse::from_embedding(model, response)).into_response()
        }
        Err(error) => OpenAiRejection::from(&error).into_response(),
    }
}

/// Mount the `OpenAI` embeddings route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/embeddings",
        post_with(handle, |operation| {
            operation
                .summary("OpenAI embeddings")
                .tag("openai")
                .response::<200, Json<OpenAiEmbeddingResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        }),
    )
}
