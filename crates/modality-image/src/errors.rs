//! Typed image-input diagnostics.

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// ErrorKind: Classifies image failures without transport semantics.
// -----------------------------------------------------------------------------

/// Broad image-input failure category exposed to chat adapters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorKind {
    /// Image input is malformed or unsupported.
    InvalidInput,
    /// Image input exceeds a fixed safety bound.
    Limit,
}

// -----------------------------------------------------------------------------
// Error: Describes invalid or excessive image input.
// -----------------------------------------------------------------------------

/// Bounded image-source or decoding failure.
#[derive(Debug, Diagnostic, Error)]
pub enum Error {
    /// A provider image object supplied neither accepted source field.
    #[error("image source is required")]
    #[diagnostic(code(eliza::image::missing_source))]
    MissingSource,

    /// A provider image object supplied mutually exclusive source fields.
    #[error("image must use exactly one source")]
    #[diagnostic(code(eliza::image::conflicting_sources))]
    ConflictingSources,

    /// An inline or provider reference was empty.
    #[error("image source must not be empty")]
    #[diagnostic(code(eliza::image::empty_source))]
    EmptySource,

    /// A URL did not contain a supported absolute scheme.
    #[error("image URL must use data, http, or https")]
    #[diagnostic(code(eliza::image::unsupported_url_scheme))]
    UnsupportedUrlScheme,

    /// A data URL did not contain one base64 image payload.
    #[error("image data URL must be `data:<media-type>;base64,<data>`")]
    #[diagnostic(code(eliza::image::invalid_data_url))]
    InvalidDataUrl,

    /// A declared media type is outside the PNG/JPEG subset.
    #[error("unsupported image media type `{media_type}`; expected image/png or image/jpeg")]
    #[diagnostic(code(eliza::image::unsupported_media_type))]
    UnsupportedMediaType {
        /// Rejected media type.
        media_type: String,
    },

    /// One base64 payload exceeded its encoded-size bound.
    #[error("encoded image is too large: {actual} > {limit} bytes")]
    #[diagnostic(code(eliza::image::encoded_too_large))]
    EncodedTooLarge {
        /// Submitted base64 byte count.
        actual: usize,
        /// Maximum accepted base64 byte count.
        limit: usize,
    },

    /// An inline payload was not valid standard base64.
    #[error("invalid image base64: {detail}")]
    #[diagnostic(code(eliza::image::invalid_base64))]
    InvalidBase64 {
        /// Stable decoder detail.
        detail: String,
    },

    /// Decoded bytes identified a format other than PNG or JPEG.
    #[error("unsupported image format; expected PNG or JPEG")]
    #[diagnostic(code(eliza::image::unsupported_format))]
    UnsupportedFormat,

    /// PNG or JPEG bytes could not be decoded.
    #[error("malformed image: {detail}")]
    #[diagnostic(code(eliza::image::malformed))]
    MalformedImage {
        /// Stable decoder detail.
        detail: String,
    },

    /// Declared and detected media formats differed.
    #[error("image media type `{declared}` does not match detected {actual}")]
    #[diagnostic(code(eliza::image::media_type_mismatch))]
    MediaTypeMismatch {
        /// Provider-declared media type.
        declared: &'static str,
        /// Format detected from the payload.
        actual: &'static str,
    },

    /// One decoded dimension exceeded its bound.
    #[error("image dimensions are too large: {width}x{height}; maximum dimension is {limit}")]
    #[diagnostic(code(eliza::image::dimensions_too_large))]
    DimensionsTooLarge {
        /// Decoded width.
        width: u32,
        /// Decoded height.
        height: u32,
        /// Maximum accepted width or height.
        limit: u32,
    },

    /// One image exceeded its decoded-area bound.
    #[error("decoded image area is too large: {actual} > {limit} pixels")]
    #[diagnostic(code(eliza::image::area_too_large))]
    AreaTooLarge {
        /// Decoded pixel count.
        actual: u64,
        /// Maximum accepted pixel count.
        limit: u64,
    },

    /// All decoded images together exceeded the request-area bound.
    #[error("total decoded image area is too large: {actual} > {limit} pixels")]
    #[diagnostic(code(eliza::image::total_area_too_large))]
    TotalAreaTooLarge {
        /// Aggregate decoded pixel count.
        actual: u64,
        /// Maximum accepted aggregate pixel count.
        limit: u64,
    },

    /// A request contained more images than the fixed count bound.
    #[error("too many images: {actual} > {limit}")]
    #[diagnostic(code(eliza::image::too_many))]
    TooManyImages {
        /// Submitted image count when the limit was crossed.
        actual: usize,
        /// Maximum accepted image count.
        limit: usize,
    },
}

impl Error {
    /// Classify the failure without imposing provider or HTTP semantics.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::EncodedTooLarge { .. }
            | Self::DimensionsTooLarge { .. }
            | Self::AreaTooLarge { .. }
            | Self::TotalAreaTooLarge { .. }
            | Self::TooManyImages { .. } => ErrorKind::Limit,
            Self::MissingSource
            | Self::ConflictingSources
            | Self::EmptySource
            | Self::UnsupportedUrlScheme
            | Self::InvalidDataUrl
            | Self::UnsupportedMediaType { .. }
            | Self::InvalidBase64 { .. }
            | Self::UnsupportedFormat
            | Self::MalformedImage { .. }
            | Self::MediaTypeMismatch { .. } => ErrorKind::InvalidInput,
        }
    }
}
