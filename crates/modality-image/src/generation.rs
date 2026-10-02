//! Deterministic cellular-automaton PNG generation.

use std::num::NonZeroUsize;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder as _};
use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// Model: Defines the fixed fixture identity and batch bound.
// -----------------------------------------------------------------------------

/// Provider-visible identifier for deterministic image generation.
pub const MODEL_ID: &str = "eliza-retro-image";

/// Maximum number of images returned by one request.
pub const MODEL_MAX_IMAGES: i64 = 4;

// -----------------------------------------------------------------------------
// Error: Classifies provider-neutral generation failures.
// -----------------------------------------------------------------------------

/// Broad image-generation failure category exposed to provider adapters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorKind {
    /// Request content is missing or invalid.
    InvalidInput,
    /// Request content exceeds a configured bound.
    Limit,
    /// PNG encoding failed after request validation.
    Internal,
}

/// Provider-neutral request field associated with a generation failure.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorField {
    /// Text prompt.
    Prompt,
    /// Requested image count.
    Count,
    /// No request field; generation failed internally.
    None,
}

/// Failure raised by deterministic image generation.
#[derive(Debug, Diagnostic, Error)]
pub enum Error {
    /// Prompt became empty after whitespace normalization.
    #[error("image prompt must not be blank")]
    #[diagnostic(code(eliza::image::generation::blank_prompt))]
    BlankPrompt,

    /// Prompt text exceeded the configured character bound.
    #[error("image prompt is too large: {actual} > {limit}")]
    #[diagnostic(code(eliza::image::generation::prompt_too_large))]
    PromptTooLarge {
        /// Submitted prompt character count.
        actual: usize,
        /// Configured maximum character count.
        limit: usize,
    },

    /// Image count fell outside the fixed one-through-four range.
    #[error("image count must be between 1 and {MODEL_MAX_IMAGES}, got {actual}")]
    #[diagnostic(code(eliza::image::generation::invalid_count))]
    InvalidCount {
        /// Rejected image count.
        actual: i64,
    },

    /// The fixed PNG encoder failed.
    #[error("failed to encode generated PNG: {detail}")]
    #[diagnostic(code(eliza::image::generation::encoding_failed))]
    Encoding {
        /// Stable encoder failure detail.
        detail: String,
    },
}

impl Error {
    /// Classify the failure without imposing provider or HTTP semantics.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::BlankPrompt | Self::InvalidCount { .. } => ErrorKind::InvalidInput,
            Self::PromptTooLarge { .. } => ErrorKind::Limit,
            Self::Encoding { .. } => ErrorKind::Internal,
        }
    }

    /// Identify the provider-neutral request field associated with this failure.
    #[must_use]
    pub const fn field(&self) -> ErrorField {
        match self {
            Self::BlankPrompt | Self::PromptTooLarge { .. } => ErrorField::Prompt,
            Self::InvalidCount { .. } => ErrorField::Count,
            Self::Encoding { .. } => ErrorField::None,
        }
    }
}

// -----------------------------------------------------------------------------
// Dimensions: Names width and height without positional tuple semantics.
// -----------------------------------------------------------------------------

/// Exact output dimensions.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Dimensions {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

// -----------------------------------------------------------------------------
// Size: Restricts output work to five fixed dimensions.
// -----------------------------------------------------------------------------

/// Supported output dimensions.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Size {
    /// 256-by-256 square.
    Square256,
    /// 512-by-512 square.
    Square512,
    /// 1024-by-1024 square.
    Square1024,
    /// 1536-by-1024 landscape image.
    Landscape1536,
    /// 1024-by-1536 portrait image.
    Portrait1536,
}

impl Size {
    /// Return exact output dimensions.
    #[must_use]
    pub const fn dimensions(self) -> Dimensions {
        match self {
            Self::Square256 => Dimensions {
                width: 256,
                height: 256,
            },
            Self::Square512 => Dimensions {
                width: 512,
                height: 512,
            },
            Self::Square1024 => Dimensions {
                width: 1024,
                height: 1024,
            },
            Self::Landscape1536 => Dimensions {
                width: 1536,
                height: 1024,
            },
            Self::Portrait1536 => Dimensions {
                width: 1024,
                height: 1536,
            },
        }
    }

    /// Return the provider spelling shared by `OpenAI` responses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Square256 => "256x256",
            Self::Square512 => "512x512",
            Self::Square1024 => "1024x1024",
            Self::Landscape1536 => "1536x1024",
            Self::Portrait1536 => "1024x1536",
        }
    }
}

// -----------------------------------------------------------------------------
// Request: Validates and completes one provider-neutral generation request.
// -----------------------------------------------------------------------------

/// One bounded image-generation request.
#[derive(Debug)]
pub struct Request {
    /// Provider prompt before whitespace normalization.
    pub prompt: String,
    /// Number of ordered variations to render.
    pub count: i64,
    /// Fixed output dimensions.
    pub size: Size,
}

impl Request {
    /// Validate, normalize, and render every requested variation.
    ///
    /// # Errors
    ///
    /// Returns a typed invalid-input, limit, or PNG-encoding failure.
    pub fn complete(self, max_input_chars: NonZeroUsize) -> Result<Response, Error> {
        // Reject invalid batch sizes before normalizing or allocating output.
        if !(1..=MODEL_MAX_IMAGES).contains(&self.count) {
            return Err(Error::InvalidCount { actual: self.count });
        }
        let prompt = Prompt::from_raw(&self.prompt, max_input_chars)?;
        let count =
            usize::try_from(self.count).map_err(|_| Error::InvalidCount { actual: self.count })?;

        // Render variations in stable zero-based order.
        let mut images = Vec::with_capacity(count);
        for variation in 0..count {
            let variation =
                u64::try_from(variation).map_err(|_| Error::InvalidCount { actual: self.count })?;
            images.push(prompt.render(variation, self.size)?);
        }
        Ok(Response {
            normalized_prompt: prompt.normalized,
            images,
            prompt_tokens: prompt.tokens,
        })
    }
}

// -----------------------------------------------------------------------------
// Response: Returns ordered PNGs and prompt accounting.
// -----------------------------------------------------------------------------

/// Ordered image-generation result.
#[derive(Debug)]
pub struct Response {
    /// Whitespace-normalized prompt used for every variation.
    pub normalized_prompt: String,
    /// PNG byte sequences ordered by variation index.
    pub images: Vec<Vec<u8>>,
    /// Approximate whitespace-delimited prompt tokens.
    pub prompt_tokens: usize,
}

// -----------------------------------------------------------------------------
// Prompt: Owns normalized text and its stable hash input.
// -----------------------------------------------------------------------------

/// Validated whitespace-normalized generation prompt.
struct Prompt {
    /// Canonical prompt text.
    normalized: String,
    /// Approximate whitespace-delimited token count.
    tokens: usize,
}

impl Prompt {
    /// Validate raw text and collapse every Unicode whitespace run.
    ///
    /// # Errors
    ///
    /// Returns an error when the raw prompt exceeds the configured character
    /// limit or normalizes to an empty string.
    fn from_raw(raw: &str, max_input_chars: NonZeroUsize) -> Result<Self, Error> {
        let input_chars = raw.chars().count();

        // Enforce the configured raw input bound before allocating normalized text.
        if input_chars > max_input_chars.get() {
            return Err(Error::PromptTooLarge {
                actual: input_chars,
                limit: max_input_chars.get(),
            });
        }
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");

        // Whitespace alone cannot select a meaningful deterministic fixture.
        if normalized.is_empty() {
            return Err(Error::BlankPrompt);
        }
        let tokens = normalized.split_whitespace().count();
        Ok(Self { normalized, tokens })
    }

    /// Hash normalized bytes followed by a little-endian variation index.
    fn hash(&self, variation: u64) -> u64 {
        let mut hash = HASH_FNV_OFFSET;
        for byte in self.normalized.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(HASH_FNV_PRIME);
        }
        for byte in variation.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(HASH_FNV_PRIME);
        }
        hash
    }

    /// Render one deterministic PNG variation.
    ///
    /// # Errors
    ///
    /// Returns an internal error when PNG encoding fails.
    fn render(&self, variation: u64, size: Size) -> Result<Vec<u8>, Error> {
        // Select fixture behavior directly from stable hash bits.
        let hash = self.hash(variation);
        let selector = hash.to_le_bytes()[0];
        let rule = AUTOMATON_RULES[usize::from(selector & 0b11)];
        let palette = PALETTE_COLORS[usize::from((selector >> 2) & 0b11)];

        // Evolve the field and reserve its scaled RGB output.
        let field = automaton_render(hash, rule);
        let dimensions = size.dimensions();
        let output_pixels = u64::from(dimensions.width) * u64::from(dimensions.height);
        let output_bytes = output_pixels * u64::from(ExtendedColorType::Rgb8.channel_count());
        let capacity = usize::try_from(output_bytes).unwrap_or(0);
        let mut pixels = Vec::with_capacity(capacity);

        // Map every output coordinate back to the nearest source cell.
        for y in 0..dimensions.height {
            let source_y = u64::from(y) * AUTOMATON_SIDE_U64 / u64::from(dimensions.height);
            for x in 0..dimensions.width {
                let source_x = u64::from(x) * AUTOMATON_SIDE_U64 / u64::from(dimensions.width);
                let field_index = source_y * AUTOMATON_SIDE_U64 + source_x;
                let field_index = usize::try_from(field_index).unwrap_or(0);
                let color = palette[usize::from(field[field_index])];
                pixels.extend_from_slice(&color);
            }
        }

        // Configure the fixed encoder without adaptive filters or metadata.
        let mut bytes = Vec::new();
        let encoder =
            PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::NoFilter);
        let color_type = ExtendedColorType::Rgb8;

        // Encode the complete image before returning its owned bytes.
        encoder
            .write_image(&pixels, dimensions.width, dimensions.height, color_type)
            .map_err(|source| Error::Encoding {
                detail: source.to_string(),
            })?;
        Ok(bytes)
    }
}

// -----------------------------------------------------------------------------
// Hash: Defines fixed FNV-1a and xorshift behavior.
// -----------------------------------------------------------------------------

/// Standard 64-bit FNV-1a offset basis.
const HASH_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Standard 64-bit FNV-1a prime.
const HASH_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Advance the fixed xorshift64 sequence once.
const fn hash_xorshift(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

// -----------------------------------------------------------------------------
// Automaton: Builds one wrapped 256-by-256 elementary field.
// -----------------------------------------------------------------------------

/// Side length of the cellular-automaton field.
const AUTOMATON_SIDE: usize = 256;

/// Side length represented for scaling arithmetic.
const AUTOMATON_SIDE_U64: u64 = 256;

/// Number of cells in the complete cellular-automaton field.
const AUTOMATON_CELLS: usize = AUTOMATON_SIDE * AUTOMATON_SIDE;

/// Cellular-automaton rules selected by the lowest two hash bits.
const AUTOMATON_RULES: [u8; 4] = [30, 45, 90, 110];

/// Render the selected elementary cellular automaton into zero-or-one cells.
fn automaton_render(hash: u64, rule: u8) -> Vec<u8> {
    let mut cells = vec![0_u8; AUTOMATON_CELLS];
    let mut state = if hash == 0 { HASH_FNV_OFFSET } else { hash };

    // Fill the first row from four successive 64-bit xorshift outputs.
    for block in 0..4 {
        state = hash_xorshift(state);
        for bit in 0..64 {
            let index = block * 64 + bit;
            cells[index] = u8::from(state & (1_u64 << bit) != 0);
        }
    }

    // Evolve remaining rows with wrapped left and right neighbors.
    for row in 1..AUTOMATON_SIDE {
        let previous = (row - 1) * AUTOMATON_SIDE;
        let current = row * AUTOMATON_SIDE;
        for column in 0..AUTOMATON_SIDE {
            let left = cells[previous + (column + AUTOMATON_SIDE - 1) % AUTOMATON_SIDE];
            let center = cells[previous + column];
            let right = cells[previous + (column + 1) % AUTOMATON_SIDE];
            let neighborhood = left * 4 + center * 2 + right;
            cells[current + column] = (rule >> neighborhood) & 1;
        }
    }
    cells
}

// -----------------------------------------------------------------------------
// PaletteColors: Defines the four literal two-color mappings.
// -----------------------------------------------------------------------------

/// Literal RGB interpretations of the documented retro palettes.
const PALETTE_COLORS: [[[u8; 3]; 2]; 4] = [
    [[0x00, 0x00, 0x00], [0xff, 0x00, 0xff]],
    [[0x00, 0x00, 0x80], [0x00, 0xff, 0xff]],
    [[0x00, 0x00, 0x00], [0x00, 0xff, 0x00]],
    [[0x80, 0x00, 0x20], [0xff, 0xff, 0x00]],
];

// -----------------------------------------------------------------------------
// Tests: Pin deterministic selection, evolution, dimensions, and failures.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
#[expect(
    rlib::missing_section_dividers,
    reason = "the compact scenario tests intentionally use behavioral names"
)]
mod tests {
    use image::ImageReader;

    use super::*;

    /// Complete one compact fixture request.
    fn complete(prompt: &str, count: i64, size: Size) -> Response {
        Request {
            prompt: prompt.to_owned(),
            count,
            size,
        }
        .complete(NonZeroUsize::new(8_000).unwrap())
        .unwrap()
    }

    /// Compute a stable byte-level golden digest.
    fn byte_hash(bytes: &[u8]) -> u64 {
        let mut hash = HASH_FNV_OFFSET;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(HASH_FNV_PRIME);
        }
        hash
    }

    #[test]
    fn it_should_normalize_prompt_whitespace() {
        let response = complete("  neon\n\t dream  ", 1, Size::Square256);

        assert_eq!(response.normalized_prompt, "neon dream");
        assert_eq!(response.prompt_tokens, 2);
    }

    #[test]
    fn it_should_reject_blank_large_and_invalid_count_requests() {
        let blank = Request {
            prompt: " \n\t".to_owned(),
            count: 1,
            size: Size::Square256,
        }
        .complete(NonZeroUsize::new(8).unwrap());
        assert!(matches!(blank, Err(Error::BlankPrompt)));

        let large = Request {
            prompt: "12345".to_owned(),
            count: 1,
            size: Size::Square256,
        }
        .complete(NonZeroUsize::new(4).unwrap());
        assert!(matches!(large, Err(Error::PromptTooLarge { .. })));

        for count in [0, 5, -1] {
            let invalid = Request {
                prompt: "valid".to_owned(),
                count,
                size: Size::Square256,
            }
            .complete(NonZeroUsize::new(8).unwrap());
            assert!(matches!(invalid, Err(Error::InvalidCount { actual }) if actual == count));
        }
    }

    #[test]
    fn it_should_render_every_supported_dimension_as_rgb_png() {
        let sizes = [
            Size::Square256,
            Size::Square512,
            Size::Square1024,
            Size::Landscape1536,
            Size::Portrait1536,
        ];
        for size in sizes {
            let response = complete("dimensions", 1, size);
            let reader = ImageReader::new(std::io::Cursor::new(&response.images[0]));
            let reader = reader.with_guessed_format().unwrap();
            let image = reader.decode().unwrap();
            let dimensions = size.dimensions();
            assert_eq!(image.width(), dimensions.width);
            assert_eq!(image.height(), dimensions.height);
            assert!(response.images[0].starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }

    #[test]
    fn it_should_repeat_bytes_and_vary_ordered_images() {
        let first = complete("repeatable", 2, Size::Square256);
        let second = complete("repeatable", 2, Size::Square256);

        assert_eq!(first.images, second.images);
        assert_ne!(first.images[0], first.images[1]);
    }

    #[test]
    fn it_should_pin_rule_palette_and_png_bytes() {
        let prompt = Prompt::from_raw("retro cats dream", NonZeroUsize::MAX).unwrap();
        let hash = prompt.hash(0);
        let selector = hash.to_le_bytes()[0];
        assert_eq!(AUTOMATON_RULES[usize::from(selector & 0b11)], 110);
        assert_eq!(
            PALETTE_COLORS[usize::from((selector >> 2) & 0b11)],
            [[0x00, 0x00, 0x80], [0x00, 0xff, 0xff]]
        );

        let response = complete("retro cats dream", 1, Size::Square256);
        assert_eq!(byte_hash(&response.images[0]), 0x4835_8837_956f_cd0f);
    }

    #[test]
    fn it_should_wrap_automaton_edges() {
        let field = automaton_render(1, 90);
        assert_eq!(field.len(), AUTOMATON_CELLS);
        assert!(field.iter().all(|cell| *cell <= 1));
        assert_ne!(
            &field[..AUTOMATON_SIDE],
            &field[AUTOMATON_SIDE..2 * AUTOMATON_SIDE]
        );
    }
}
