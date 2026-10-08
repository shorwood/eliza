//! Small deterministic embedding engine shared by provider adapters.

use std::num::NonZeroUsize;

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// Error: Classifies failures without transport semantics.
// -----------------------------------------------------------------------------

/// Broad failure category exposed to provider adapters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorKind {
    /// Provider input cannot be embedded.
    InvalidInput,
    /// Provider input exceeds a configured bound.
    Limit,
}

/// Provider-neutral request field associated with a failure.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorField {
    /// Text input or batch contents.
    Input,
    /// Requested vector dimensions.
    Dimensions,
}

/// Typed failures raised by provider-neutral embedding handling.
#[derive(Debug, Diagnostic, Error)]
pub enum Error {
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

impl Error {
    /// Classify the failure without imposing HTTP or provider semantics.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::InputTooLarge { .. } => ErrorKind::Limit,
            Self::EmptyBatch | Self::BlankInput { .. } | Self::InvalidDimensions { .. } => {
                ErrorKind::InvalidInput
            }
        }
    }

    /// Identify the provider-neutral request field associated with this failure.
    #[must_use]
    pub const fn field(&self) -> ErrorField {
        match self {
            Self::InvalidDimensions { .. } => ErrorField::Dimensions,
            Self::EmptyBatch | Self::BlankInput { .. } | Self::InputTooLarge { .. } => {
                ErrorField::Input
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Input: Defines one provider-neutral embedding input.
// -----------------------------------------------------------------------------

/// One text value and requested vector size.
#[derive(Debug)]
pub struct Input {
    /// Text to embed.
    pub text: String,
    /// Number of output vector elements.
    pub dimensions: i64,
}

impl Input {
    /// Validate and convert the requested vector size.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDimensions`] when the size is outside the model bound.
    fn dimensions(&self) -> Result<NonZeroUsize, Error> {
        // Every vector must have a bounded nonzero bucket count.
        if !(1..=MODEL_MAX_DIMENSIONS).contains(&self.dimensions) {
            return Err(Error::InvalidDimensions {
                actual: self.dimensions,
            });
        }

        let dimensions =
            usize::try_from(self.dimensions).map_err(|_| Error::InvalidDimensions {
                actual: self.dimensions,
            })?;
        NonZeroUsize::new(dimensions).ok_or(Error::InvalidDimensions {
            actual: self.dimensions,
        })
    }
}

// -----------------------------------------------------------------------------
// Response: Returns ordered vectors and usage.
// -----------------------------------------------------------------------------

/// Provider-neutral successful embedding result.
#[derive(Debug, PartialEq)]
pub struct Response {
    /// Vectors in request order.
    pub embeddings: Vec<Vec<f32>>,
    /// Approximate whitespace-delimited input tokens.
    pub prompt_tokens: usize,
}

// -----------------------------------------------------------------------------
// Model: Publishes fixed embedding model metadata.
// -----------------------------------------------------------------------------

/// Provider-visible identifier for the feature-hashing model.
pub const MODEL_ID: &str = "fnv-embed";

/// Vector size used when a provider request omits dimensions.
pub const MODEL_DEFAULT_DIMENSIONS: i64 = 256;

/// Largest vector size accepted by the bounded fixture.
pub const MODEL_MAX_DIMENSIONS: i64 = 1_024;

// -----------------------------------------------------------------------------
// FeatureVector: Normalizes and hashes lexical features into unit vectors.
// -----------------------------------------------------------------------------

/// Nonempty vector under construction for one embedding input.
struct FeatureVector(
    /// Hashed feature buckets, always nonempty.
    Vec<f32>,
);

impl FeatureVector {
    /// Hash concatenated feature fragments with 64-bit FNV-1a.
    fn hash(parts: &[&[u8]]) -> u64 {
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
    fn add(&mut self, parts: &[&[u8]]) {
        let hash = Self::hash(parts);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the remainder is smaller than the validated vector length"
        )]
        let bucket = (hash % self.0.len() as u64) as usize;
        let sign = if hash & (1_u64 << 63) == 0 { 1.0 } else { -1.0 };
        self.0[bucket] += sign;
    }

    /// Scale a nonzero vector to unit length.
    fn normalize(&mut self) {
        let squared = self.0.iter().map(|value| value * value);
        let magnitude = squared.sum::<f32>().sqrt();

        // A zero vector is already normalized and cannot be scaled.
        if magnitude == 0.0 {
            return;
        }
        for value in &mut self.0 {
            *value /= magnitude;
        }
    }

    /// Hash lexical features into one requested vector size.
    fn for_words(words: &[String], dimensions: NonZeroUsize) -> Self {
        let mut vector = Self(vec![0.0; dimensions.get()]);

        for word in words {
            vector.add(&[b"w:", word.as_bytes()]);

            let padded = format!("^{word}$");
            for trigram in padded.as_bytes().windows(3) {
                vector.add(&[b"c:", trigram]);
            }
        }
        for pair in words.windows(2) {
            vector.add(&[b"b:", pair[0].as_bytes(), b" ", pair[1].as_bytes()]);
        }

        vector.normalize();
        vector
    }

    /// Consume the wrapper after construction is complete.
    fn into_inner(self) -> Vec<f32> {
        self.0
    }
}

/// Normalize text into the deliberately small ASCII lexical model.
fn normalize_words(text: &str) -> Vec<String> {
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

// -----------------------------------------------------------------------------
// Request: Validates bounded batches and produces ordered vectors.
// -----------------------------------------------------------------------------

/// Provider-neutral embedding request.
#[derive(Debug)]
pub struct Request {
    /// Ordered text inputs.
    pub inputs: Vec<Input>,
}

impl Request {
    /// Validate and embed every input under the configured character bound.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for the wrong model, empty or oversized input,
    /// blank normalized text, or dimensions outside `1..=1024`.
    pub fn complete(self, max_input_chars: NonZeroUsize) -> Result<Response, Error> {
        // A successful response must contain at least one vector.
        if self.inputs.is_empty() {
            return Err(Error::EmptyBatch);
        }

        // Bounds keep allocations predictable across every adapter.
        let dimensions = self
            .inputs
            .iter()
            .map(Input::dimensions)
            .collect::<Result<Vec<_>, _>>()?;

        let input_chars = self.inputs.iter().fold(0_usize, |total, input| {
            total.saturating_add(input.text.chars().count())
        });

        // Provider batches share the server's aggregate input bound.
        if input_chars > max_input_chars.get() {
            return Err(Error::InputTooLarge {
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
        let indexed_inputs = self.inputs.iter().zip(dimensions).enumerate();
        let embeddings = indexed_inputs
            .map(|(index, (input, dimensions))| {
                let words = normalize_words(&input.text);

                // Punctuation-only input carries no lexical signal.
                if words.is_empty() {
                    return Err(Error::BlankInput { index });
                }
                Ok(FeatureVector::for_words(&words, dimensions).into_inner())
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Response {
            embeddings,
            prompt_tokens,
        })
    }
}
