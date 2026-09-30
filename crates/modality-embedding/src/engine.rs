//! Small deterministic embedding engine shared by provider adapters.

use std::num::NonZeroUsize;

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// Embedding: Defines provider-neutral values and fixed model metadata.
// -----------------------------------------------------------------------------

/// One text value and requested vector size.
#[derive(Debug)]
pub struct EmbeddingInput {
    /// Text to embed.
    pub text: String,
    /// Number of output vector elements.
    pub dimensions: i64,
}

/// Provider-neutral successful embedding result.
#[derive(Debug, PartialEq)]
pub struct EmbeddingResponse {
    /// Vectors in request order.
    pub embeddings: Vec<Vec<f32>>,
    /// Approximate whitespace-delimited input tokens.
    pub prompt_tokens: usize,
}

/// Typed failures raised by provider-neutral embedding handling.
#[derive(Debug, Diagnostic, Error)]
pub enum EmbeddingError {
    /// A provider requested a model other than the fixed embedding model.
    #[error("embeddings require model `fnv-embed`")]
    #[diagnostic(code(eliza::embedding::model_required))]
    ModelRequired,

    /// A batch contained no text inputs.
    #[error("embedding input batch must not be empty")]
    #[diagnostic(code(eliza::embedding::empty_batch))]
    EmptyBatch,

    /// One input became empty after lexical normalization.
    #[error("embedding input at index {index} must not be blank")]
    #[diagnostic(code(eliza::embedding::blank_input))]
    BlankInput {
        /// Zero-based batch position of the blank text.
        index: usize,
    },

    /// A requested vector size was outside the fixed range.
    #[error("embedding dimensions must be between 1 and 1024, got {actual}")]
    #[diagnostic(code(eliza::embedding::invalid_dimensions))]
    InvalidDimensions {
        /// Rejected dimension value.
        actual: i64,
    },

    /// Combined input text exceeded the configured character bound.
    #[error("embedding input is too large: {actual} > {limit}")]
    #[diagnostic(code(eliza::embedding::input_too_large))]
    InputTooLarge {
        /// Submitted character count.
        actual: usize,
        /// Configured maximum character count.
        limit: usize,
    },
}

/// Provider-visible identifier for the feature-hashing model.
pub const EMBEDDING_MODEL_ID: &str = "fnv-embed";

/// Vector size used when a provider request omits dimensions.
pub const EMBEDDING_DEFAULT_DIMENSIONS: i64 = 256;

/// Largest vector size accepted by the bounded fixture.
pub const EMBEDDING_MAX_DIMENSIONS: i64 = 1_024;

// -----------------------------------------------------------------------------
// EmbeddingFeature: Normalizes and hashes lexical features into unit vectors.
// -----------------------------------------------------------------------------

/// Normalize text into the deliberately small ASCII lexical model.
fn embedding_feature_normalize_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();

    for character in text.chars() {
        let normalized = match character {
            '\u{2018}' | '\u{2019}' => Some('\''),
            character if character.is_ascii_alphanumeric() || character == '\'' => {
                Some(character.to_ascii_lowercase())
            }
            _ => None,
        };
        if let Some(character) = normalized {
            word.push(character);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Hash concatenated feature fragments with 64-bit FNV-1a.
fn embedding_feature_hash(parts: &[&[u8]]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// Add one signed hashed feature to its vector bucket.
fn embedding_feature_add(vector: &mut [f32], parts: &[&[u8]]) {
    let hash = embedding_feature_hash(parts);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the remainder is smaller than the validated vector length"
    )]
    let bucket = (hash % vector.len() as u64) as usize;
    let sign = if hash & (1_u64 << 63) == 0 { 1.0 } else { -1.0 };
    vector[bucket] += sign;
}

/// Scale a nonzero vector to unit length.
fn embedding_feature_normalize(vector: &mut [f32]) {
    let squared = vector.iter().map(|value| value * value);
    let magnitude = squared.sum::<f32>().sqrt();

    // A zero vector is already normalized and cannot be scaled.
    if magnitude == 0.0 {
        return;
    }
    for value in vector {
        *value /= magnitude;
    }
}

/// Hash lexical features into one requested vector size.
fn embedding_feature_vector(words: &[String], dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0.0; dimensions];

    for word in words {
        embedding_feature_add(&mut vector, &[b"w:", word.as_bytes()]);

        let padded = format!("^{word}$");
        for trigram in padded.as_bytes().windows(3) {
            embedding_feature_add(&mut vector, &[b"c:", trigram]);
        }
    }
    for pair in words.windows(2) {
        embedding_feature_add(
            &mut vector,
            &[b"b:", pair[0].as_bytes(), b" ", pair[1].as_bytes()],
        );
    }

    embedding_feature_normalize(&mut vector);
    vector
}

// -----------------------------------------------------------------------------
// EmbeddingRequest: Validates bounded batches and produces ordered vectors.
// -----------------------------------------------------------------------------

/// Provider-neutral embedding request.
#[derive(Debug)]
pub struct EmbeddingRequest {
    /// Ordered text inputs.
    pub inputs: Vec<EmbeddingInput>,
}

impl EmbeddingRequest {
    /// Validate and embed every input under the configured character bound.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for the wrong model, empty or oversized input,
    /// blank normalized text, or dimensions outside `1..=1024`.
    pub fn complete(
        self,
        model: &str,
        max_input_chars: NonZeroUsize,
    ) -> Result<EmbeddingResponse, EmbeddingError> {
        // This fixture implements one fixed embedding model.
        if model != EMBEDDING_MODEL_ID {
            return Err(EmbeddingError::ModelRequired);
        }

        // A successful response must contain at least one vector.
        if self.inputs.is_empty() {
            return Err(EmbeddingError::EmptyBatch);
        }
        for input in &self.inputs {
            // Bounds keep allocations predictable across every adapter.
            if !(1..=EMBEDDING_MAX_DIMENSIONS).contains(&input.dimensions) {
                return Err(EmbeddingError::InvalidDimensions {
                    actual: input.dimensions,
                });
            }
        }

        let input_chars = self.inputs.iter().fold(0_usize, |total, input| {
            total.saturating_add(input.text.chars().count())
        });

        // Provider batches share the server's aggregate input bound.
        if input_chars > max_input_chars.get() {
            return Err(EmbeddingError::InputTooLarge {
                actual: input_chars,
                limit: max_input_chars.get(),
            });
        }

        // Account usage from the original provider text.
        let prompt_tokens = self
            .inputs
            .iter()
            .flat_map(|input| input.text.split_whitespace())
            .count();

        // Hash each normalized input without disturbing batch order.
        let indexed_inputs = self.inputs.iter().enumerate();
        let embeddings = indexed_inputs
            .map(|(index, input)| {
                let words = embedding_feature_normalize_words(&input.text);

                // Punctuation-only input carries no lexical signal.
                if words.is_empty() {
                    return Err(EmbeddingError::BlankInput { index });
                }
                let dimensions = usize::try_from(input.dimensions).map_err(|_| {
                    EmbeddingError::InvalidDimensions {
                        actual: input.dimensions,
                    }
                })?;
                Ok(embedding_feature_vector(&words, dimensions))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(EmbeddingResponse {
            embeddings,
            prompt_tokens,
        })
    }
}

// -----------------------------------------------------------------------------
// Tests: Lock down hashing, bounds, and deterministic similarity.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
mod tests {
    use super::*;

    /// Build a request containing one text value.
    fn it_should_fixture_request(text: &str, dimensions: i64) -> EmbeddingRequest {
        EmbeddingRequest {
            inputs: vec![EmbeddingInput {
                text: text.to_owned(),
                dimensions,
            }],
        }
    }

    /// Compute the cosine similarity of equal-length vectors.
    fn it_should_fixture_cosine(left: &[f32], right: &[f32]) -> f32 {
        let products = left.iter().zip(right).map(|(left, right)| left * right);
        products.sum()
    }

    #[test]
    fn it_should_normalize_ascii_words_and_curly_apostrophes() {
        assert_eq!(
            embedding_feature_normalize_words("  WE\u{2019}RE déjà--HERE 42! "),
            ["we're", "d", "j", "here", "42"]
        );
    }

    #[test]
    fn it_should_use_standard_fnv_one_a() {
        assert_eq!(embedding_feature_hash(&[b"hello"]), 0xa430_d846_80aa_bd0b);
        assert_eq!(
            embedding_feature_hash(&[b"b:", b"hello", b" ", b"world"]),
            embedding_feature_hash(&[b"b:hello world"])
        );
    }

    #[test]
    fn it_should_return_repeatable_unit_vectors() {
        let first = it_should_fixture_request("retro cats dream", EMBEDDING_DEFAULT_DIMENSIONS)
            .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
            .unwrap();
        let second = it_should_fixture_request("retro cats dream", EMBEDDING_DEFAULT_DIMENSIONS)
            .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
            .unwrap();

        assert_eq!(first, second);
        let squared = first.embeddings[0].iter().map(|value| value * value);
        let magnitude = squared.sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < f32::EPSILON.sqrt());
    }

    #[test]
    fn it_should_rank_shared_words_above_unrelated_text() {
        let related = ["retro cats dream", "retro cats sleep", "quantum engine"].map(|text| {
            it_should_fixture_request(text, EMBEDDING_DEFAULT_DIMENSIONS)
                .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
                .unwrap()
                .embeddings
                .remove(0)
        });

        assert!(
            it_should_fixture_cosine(&related[0], &related[1])
                > it_should_fixture_cosine(&related[0], &related[2])
        );
    }

    #[test]
    fn it_should_preserve_batch_order_and_per_item_dimensions() {
        let response = EmbeddingRequest {
            inputs: vec![
                EmbeddingInput {
                    text: "first text".to_owned(),
                    dimensions: 1,
                },
                EmbeddingInput {
                    text: "second text".to_owned(),
                    dimensions: EMBEDDING_MAX_DIMENSIONS,
                },
            ],
        }
        .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
        .unwrap();

        assert_eq!(response.embeddings[0].len(), 1);
        assert_eq!(response.embeddings[1].len(), 1_024);
        assert_eq!(response.prompt_tokens, 4);
    }

    #[test]
    fn it_should_reject_invalid_input_and_dimensions() {
        let empty = EmbeddingRequest { inputs: Vec::new() }
            .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
            .unwrap_err();
        assert!(matches!(empty, EmbeddingError::EmptyBatch));

        let blank = it_should_fixture_request(" ... ", EMBEDDING_DEFAULT_DIMENSIONS)
            .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
            .unwrap_err();
        assert!(matches!(blank, EmbeddingError::BlankInput { index: 0 }));

        for dimensions in [0, EMBEDDING_MAX_DIMENSIONS + 1] {
            let error = it_should_fixture_request("text", dimensions)
                .complete(EMBEDDING_MODEL_ID, NonZeroUsize::MAX)
                .unwrap_err();
            assert!(matches!(error, EmbeddingError::InvalidDimensions { .. }));
        }
    }

    #[test]
    fn it_should_enforce_the_combined_character_limit() {
        let error = it_should_fixture_request("four", EMBEDDING_DEFAULT_DIMENSIONS)
            .complete(EMBEDDING_MODEL_ID, NonZeroUsize::new(3).unwrap())
            .unwrap_err();

        assert!(matches!(
            error,
            EmbeddingError::InputTooLarge {
                actual: 4,
                limit: 3
            }
        ));
    }

    #[test]
    fn it_should_leave_zero_magnitude_vectors_unchanged() {
        let mut vector = [0.0, 0.0];
        embedding_feature_normalize(&mut vector);
        assert!(vector.iter().all(|value| value.abs() < f32::EPSILON));
    }
}
