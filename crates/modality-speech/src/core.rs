//! Provider-neutral speech requests and responses.
use std::num::NonZeroUsize;

use super::errors::SpeechError;

// -----------------------------------------------------------------------------
// ModelId: Publishes the built-in speech model identifier.
// -----------------------------------------------------------------------------

/// Provider-visible identifier for the bundled retro voice.
pub const MODEL_ID: &str = "flite";

// -----------------------------------------------------------------------------
// AudioFormat: Names the supported output encodings.
// -----------------------------------------------------------------------------

/// Audio container or sample encoding requested by a provider adapter.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AudioFormat {
    /// MPEG Layer III.
    Mp3,
    /// RIFF/WAVE containing signed 16-bit PCM.
    Wav,
    /// Headerless signed 16-bit little-endian PCM.
    Pcm,
    /// Headerless signed 16-bit big-endian PCM for RFC L16.
    L16,
    /// Headerless G.711 mu-law.
    MuLaw,
    /// Headerless G.711 A-law.
    ALaw,
}

// -----------------------------------------------------------------------------
// Speech: Defines provider-neutral synthesis input.
// -----------------------------------------------------------------------------

/// One independently voiced piece of a speech request.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    /// Text pronounced by Flite's English speech pipeline.
    pub text: String,
    /// Arbitrary provider voice identifier.
    pub voice: String,
    /// Optional provider prose interpreted as a small set of style keywords.
    pub style: String,
    /// Explicit speaking-rate multiplier.
    pub speed: f32,
    /// Silence appended after this segment.
    pub pause_after_ms: u16,
}

/// Provider-neutral speech request.
#[derive(Debug, Clone)]
pub struct SpeechRequest {
    /// Ordered text and voice segments.
    pub segments: Vec<SpeechSegment>,
    /// Output samples per second.
    pub sample_rate: u32,
}

impl SpeechRequest {
    /// Approximate input tokens using the convention used by chat adapters.
    #[must_use]
    pub fn input_tokens(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.text.split_whitespace().count())
            .sum()
    }

    /// Validate provider-neutral limits before occupying a synthesis worker.
    ///
    /// # Errors
    ///
    /// Returns a typed request failure for invalid text, rate, speed, or size.
    pub(super) fn validate(&self, max_chars: NonZeroUsize) -> Result<(), SpeechError> {
        // The synthesizer and exposed encodings intentionally support three rates.
        if !matches!(self.sample_rate, 8_000 | 16_000 | 24_000) {
            return Err(SpeechError::UnsupportedSampleRate {
                sample_rate: self.sample_rate,
            });
        }

        // A request with no pronounceable text cannot produce meaningful audio.
        if self.segments.is_empty()
            || self
                .segments
                .iter()
                .all(|segment| segment.text.trim().is_empty())
        {
            return Err(SpeechError::EmptyInput);
        }

        let chars = self
            .segments
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum::<usize>();

        // Bound parsing and synthesis work before acquiring a worker permit.
        if chars > max_chars.get() {
            return Err(SpeechError::InputTooLarge {
                actual: chars,
                limit: max_chars.get(),
            });
        }

        for segment in &self.segments {
            // Empty pieces make speaker ordering and usage accounting ambiguous.
            if segment.text.trim().is_empty() {
                return Err(SpeechError::EmptyInput);
            }

            // Keep the deliberately small English fixture surface ASCII-only.
            if let Some(character) = segment.text.chars().find(|character| {
                !character.is_ascii()
                    || (character.is_ascii_control() && !character.is_ascii_whitespace())
            }) {
                return Err(SpeechError::UnsupportedCharacter { character });
            }

            // Enforce the provider speaking-rate range.
            if !(0.25..=4.0).contains(&segment.speed) {
                return Err(SpeechError::InvalidSpeed);
            }
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// OutputTokens: Estimates generated audio usage.
// -----------------------------------------------------------------------------

/// Approximate audio tokens as 20 ms frames.
#[must_use]
pub fn output_tokens(sample_count: usize, sample_rate: u32) -> usize {
    sample_count.div_ceil((sample_rate as usize).div_ceil(50))
}

// -----------------------------------------------------------------------------
// RenderedAudio: Carries encoded engine output to provider adapters.
// -----------------------------------------------------------------------------

/// Encoded audio returned to a provider adapter.
#[derive(Debug)]
pub struct RenderedAudio {
    /// Complete encoded payload.
    pub bytes: Vec<u8>,
    /// Provider-safe MIME type including the sample rate where relevant.
    pub media_type: String,
    /// Number of uncompressed mono samples represented by the payload.
    pub(super) sample_count: usize,
    /// Samples per second in the uncompressed signal.
    pub(super) sample_rate: u32,
}

impl RenderedAudio {
    /// Approximate audio tokens as 20 ms frames.
    #[must_use]
    pub fn output_tokens(&self) -> usize {
        output_tokens(self.sample_count, self.sample_rate)
    }
}
