//! Fixed transport and decoded-image safety bounds.

// -----------------------------------------------------------------------------
// Limit: Defines fixed encoded and decoded image bounds.
// -----------------------------------------------------------------------------

/// Maximum JSON body size on routes that accept inline images.
pub const LIMIT_JSON_BODY: usize = 48 * 1024 * 1024;

/// Maximum number of images accepted in one request.
pub const LIMIT_IMAGES: usize = 4;

/// Maximum base64 characters accepted for one inline image.
pub const LIMIT_ENCODED_BYTES: usize = 8 * 1024 * 1024;

/// Maximum width or height accepted for one decoded image.
pub const LIMIT_DIMENSION: u32 = 4096;

/// Maximum decoded pixels accepted for one image.
pub const LIMIT_IMAGE_PIXELS: u64 = 4_000_000;

/// Maximum decoded pixels accepted across one request.
pub const LIMIT_REQUEST_PIXELS: u64 = 8_000_000;
