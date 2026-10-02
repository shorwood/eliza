//! Normalized inline and opaque image sources.

use std::str::FromStr;

use crate::errors::Error;

// -----------------------------------------------------------------------------
// MediaType: Restricts decoded images to PNG and JPEG.
// -----------------------------------------------------------------------------

/// Supported declared image media type.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum MediaType {
    /// Portable Network Graphics.
    Png,
    /// Joint Photographic Experts Group image.
    Jpeg,
}

impl FromStr for MediaType {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "image/png" => Ok(Self::Png),
            "image/jpeg" => Ok(Self::Jpeg),
            _ => Err(Error::UnsupportedMediaType {
                media_type: value.to_owned(),
            }),
        }
    }
}

impl MediaType {
    /// Return the canonical wire media type.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

// -----------------------------------------------------------------------------
// Source: Retains image bytes or an intentionally uninspected reference.
// -----------------------------------------------------------------------------

/// Internal representation of one normalized image source.
#[derive(Debug, Clone)]
pub(super) enum SourceKind {
    /// Base64 image data, optionally accompanied by a declared media type.
    Inline {
        /// Declared wire media type when the provider carries one.
        declared: Option<MediaType>,
        /// Standard base64 payload.
        encoded: String,
    },
    /// Remote or provider-owned source deliberately never fetched.
    Opaque,
}

/// One normalized image source awaiting request-scoped analysis.
#[derive(Debug, Clone)]
pub struct Source(
    /// Validated source representation available to the analyzer.
    pub(super) SourceKind,
);

impl Source {
    /// Retain raw provider base64 whose media type must be detected.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptySource`] when the payload is empty.
    pub fn inline_base64(encoded: String) -> Result<Self, Error> {
        Self::inline(None, encoded)
    }

    /// Retain base64 with one required PNG/JPEG media type.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an empty payload or unsupported media type.
    pub fn typed_inline_base64(media_type: &str, encoded: String) -> Result<Self, Error> {
        Self::inline(Some(media_type.parse()?), encoded)
    }

    /// Normalize a data URL or accept an HTTP(S) URL as opaque.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an unsupported scheme or malformed data URL.
    pub fn url(value: &str) -> Result<Self, Error> {
        // Data URLs become bounded inline data; no URL parser or fetch is involved.
        if let Some(data) = value.strip_prefix("data:") {
            return Self::data_url(data);
        }

        // HTTP(S) URLs terminate as opaque sources and are never fetched.
        if let Some(location) = value
            .strip_prefix("http://")
            .or_else(|| value.strip_prefix("https://"))
        {
            return if location.is_empty() {
                Err(Error::EmptySource)
            } else {
                Ok(Self(SourceKind::Opaque))
            };
        }
        Err(if value.is_empty() {
            Error::EmptySource
        } else {
            Error::UnsupportedUrlScheme
        })
    }

    /// Accept a nonempty provider-owned file reference as opaque.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an empty reference or unsupported declared
    /// media type.
    pub fn provider_reference(reference: &str, media_type: Option<&str>) -> Result<Self, Error> {
        if let Some(media_type) = media_type {
            media_type.parse::<MediaType>()?;
        }
        if reference.is_empty() {
            Err(Error::EmptySource)
        } else {
            Ok(Self(SourceKind::Opaque))
        }
    }

    /// Select exactly one URL or provider reference.
    ///
    /// # Errors
    ///
    /// Returns a typed error when both or neither source is supplied.
    pub fn url_or_reference(url: Option<String>, reference: Option<String>) -> Result<Self, Error> {
        match (url, reference) {
            (Some(url), None) => Self::url(&url),
            (None, Some(reference)) => Self::provider_reference(&reference, None),
            (None, None) => Err(Error::MissingSource),
            (Some(_), Some(_)) => Err(Error::ConflictingSources),
        }
    }

    /// Retain one validated inline representation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptySource`] when the payload is empty.
    fn inline(declared: Option<MediaType>, encoded: String) -> Result<Self, Error> {
        if encoded.is_empty() {
            Err(Error::EmptySource)
        } else {
            Ok(Self(SourceKind::Inline { declared, encoded }))
        }
    }

    /// Split one data URL into its declared type and base64 payload.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed metadata, an empty payload, or an
    /// unsupported media type.
    fn data_url(data: &str) -> Result<Self, Error> {
        let (metadata, encoded) = data.split_once(',').ok_or(Error::InvalidDataUrl)?;
        let media_type = metadata
            .strip_suffix(";base64")
            .filter(|value| !value.is_empty())
            .ok_or(Error::InvalidDataUrl)?;
        Self::typed_inline_base64(media_type, encoded.to_owned())
    }
}
