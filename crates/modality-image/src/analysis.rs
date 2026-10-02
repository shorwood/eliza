//! Fixed-grid pixel analysis and request-scoped bounds.

use std::fmt;
use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{DynamicImage, GenericImageView as _, ImageFormat, ImageReader, Rgba};

use crate::errors::Error;
use crate::limits::{
    LIMIT_DIMENSION, LIMIT_ENCODED_BYTES, LIMIT_IMAGE_PIXELS, LIMIT_IMAGES, LIMIT_REQUEST_PIXELS,
};
use crate::source::{MediaType, Source, SourceKind};

// -----------------------------------------------------------------------------
// Sample: Defines the bounded analysis grid.
// -----------------------------------------------------------------------------

/// Width and height of the deterministic sample grid.
const SAMPLE_SIDE: u32 = 64;

/// Number of sampled pixels per decoded image.
const SAMPLE_COUNT: u64 = 4096;

// -----------------------------------------------------------------------------
// Aspect: Names the coarse decoded aspect ratio.
// -----------------------------------------------------------------------------

/// Coarse decoded aspect ratio.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Aspect {
    /// Width is less than 90 percent of height.
    Portrait,
    /// Width is between 90 and 110 percent of height.
    Square,
    /// Width is greater than 110 percent of height.
    Landscape,
}

impl Aspect {
    /// Classify aspect ratio using exact forms of the 0.9 and 1.1 bounds.
    const fn for_dimensions(width: u32, height: u32) -> Self {
        if width * 10 < height * 9 {
            Self::Portrait
        } else if width * 10 > height * 11 {
            Self::Landscape
        } else {
            Self::Square
        }
    }

    /// Return the response label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "PORTRAIT",
            Self::Square => "SQUARE",
            Self::Landscape => "LANDSCAPE",
        }
    }
}

// -----------------------------------------------------------------------------
// Brightness: Names coarse mean sampled luma.
// -----------------------------------------------------------------------------

/// Coarse mean sampled luma.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Brightness {
    /// Mean luma is below 85.
    Dark,
    /// Mean luma is from 85 through 170.
    Midtone,
    /// Mean luma is above 170.
    Light,
}

impl Brightness {
    /// Classify the exact mean without floating-point rounding.
    const fn for_sum(luma_sum: u64) -> Self {
        if luma_sum < 85 * SAMPLE_COUNT {
            Self::Dark
        } else if luma_sum > 170 * SAMPLE_COUNT {
            Self::Light
        } else {
            Self::Midtone
        }
    }

    /// Return the response label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "DARK",
            Self::Midtone => "MIDTONE",
            Self::Light => "LIGHT",
        }
    }
}

// -----------------------------------------------------------------------------
// Color: Names coarse red-versus-blue balance.
// -----------------------------------------------------------------------------

/// Sampled red and blue channel totals.
#[derive(Clone, Copy)]
struct ColorSums {
    /// Sum of sampled red values.
    red: u64,
    /// Sum of sampled blue values.
    blue: u64,
}

/// Coarse mean red-versus-blue balance.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Color {
    /// Mean red exceeds mean blue by at least 12.
    Warm,
    /// Mean red and blue differ by less than 12.
    Neutral,
    /// Mean blue exceeds mean red by at least 12.
    Cool,
}

impl Color {
    /// Classify mean channel separation without rounding either mean.
    const fn for_sums(sums: ColorSums) -> Self {
        let separation = 12 * SAMPLE_COUNT;
        if sums.red >= sums.blue + separation {
            Self::Warm
        } else if sums.blue >= sums.red + separation {
            Self::Cool
        } else {
            Self::Neutral
        }
    }

    /// Return the response label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "WARM",
            Self::Neutral => "NEUTRAL",
            Self::Cool => "COOL",
        }
    }
}

// -----------------------------------------------------------------------------
// Contrast: Names coarse sampled-luma deviation.
// -----------------------------------------------------------------------------

/// First and second sampled-luma moments.
#[derive(Clone, Copy)]
struct ContrastMoments {
    /// Sum of sampled luma.
    sum: u64,
    /// Sum of squared sampled luma.
    squared_sum: u64,
}

/// Coarse sampled-luma standard deviation.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Contrast {
    /// Standard deviation is below 32.
    Low,
    /// Standard deviation is from 32 through 64.
    Medium,
    /// Standard deviation is above 64.
    High,
}

impl Contrast {
    /// Classify population variance without a square root or floating point.
    fn for_moments(moments: ContrastMoments) -> Self {
        let count = u128::from(SAMPLE_COUNT);
        let variance_numerator =
            u128::from(moments.squared_sum) * count - u128::from(moments.sum).pow(2);
        let low_bound = 32_u128.pow(2) * count.pow(2);
        let high_bound = 64_u128.pow(2) * count.pow(2);
        if variance_numerator < low_bound {
            Self::Low
        } else if variance_numerator > high_bound {
            Self::High
        } else {
            Self::Medium
        }
    }

    /// Return the response label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }
}

// -----------------------------------------------------------------------------
// Analysis: Renders one numbered deterministic vision line.
// -----------------------------------------------------------------------------

/// Internal result of one numbered image analysis.
#[derive(Debug, Clone, Eq, PartialEq)]
enum AnalysisKind {
    /// A remote URL or provider file reference was deliberately not fetched.
    Opaque {
        /// One-based request image number.
        number: usize,
    },
    /// A decoded inline image measured on the fixed sample grid.
    Inspected {
        /// One-based request image number.
        number: usize,
        /// Decoded width.
        width: u32,
        /// Decoded height.
        height: u32,
        /// Coarse aspect class.
        aspect: Aspect,
        /// Coarse mean-luma class.
        brightness: Brightness,
        /// Coarse red-versus-blue class.
        color: Color,
        /// Coarse luma-deviation class.
        contrast: Contrast,
        /// Samples at or below the Otsu threshold, rounded to a percent.
        foreground: u64,
    },
}

/// One numbered image analysis retained for replay and response rendering.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Analysis(
    /// Internal rendered-analysis fields.
    AnalysisKind,
);

impl fmt::Display for Analysis {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            AnalysisKind::Opaque { number } => write!(
                formatter,
                "[RETRO VISION {number}: OPAQUE IMAGE NOT INSPECTED]"
            ),
            AnalysisKind::Inspected {
                number,
                width,
                height,
                aspect,
                brightness,
                color,
                contrast,
                foreground,
            } => write!(
                formatter,
                "[RETRO VISION {number}: {width}x{height} {}, {}, {}, {} CONTRAST, {foreground}% FOREGROUND]",
                aspect.as_str(),
                brightness.as_str(),
                color.as_str(),
                contrast.as_str(),
            ),
        }
    }
}

impl Analysis {
    /// Analyze cell centers on the fixed grid.
    #[expect(
        rlib::missing_code_phase_comments,
        reason = "the final phase is a flat mapping from named statistics to output fields"
    )]
    fn from_pixels(number: usize, width: u32, height: u32, image: &DynamicImage) -> Self {
        let mut red_sum = 0_u64;
        let mut blue_sum = 0_u64;
        let mut luma_sum = 0_u64;
        let mut luma_squared_sum = 0_u64;
        let mut histogram = [0_u64; 256];

        // Accumulate bounded integer statistics from every sample cell center.
        for row in 0..SAMPLE_SIDE {
            let y = ((2 * row + 1) * height / (2 * SAMPLE_SIDE)).min(height - 1);
            for column in 0..SAMPLE_SIDE {
                let x = ((2 * column + 1) * width / (2 * SAMPLE_SIDE)).min(width - 1);
                let rgb = pixel_composite_white(image.get_pixel(x, y));
                let sample_luma = pixel_luma(rgb);
                red_sum += u64::from(rgb[0]);
                blue_sum += u64::from(rgb[2]);
                luma_sum += u64::from(sample_luma);
                luma_squared_sum += u64::from(sample_luma).pow(2);
                histogram[usize::from(sample_luma)] += 1;
            }
        }

        // Render classifications from the completed sample statistics.
        let threshold = otsu_threshold(&histogram);
        Self(AnalysisKind::Inspected {
            number,
            width,
            height,
            aspect: Aspect::for_dimensions(width, height),
            brightness: Brightness::for_sum(luma_sum),
            color: Color::for_sums(ColorSums {
                red: red_sum,
                blue: blue_sum,
            }),
            contrast: Contrast::for_moments(ContrastMoments {
                sum: luma_sum,
                squared_sum: luma_squared_sum,
            }),
            foreground: otsu_foreground_percentage(&histogram, threshold),
        })
    }
}

// -----------------------------------------------------------------------------
// Batch: Enforces request-wide image count and decoded area.
// -----------------------------------------------------------------------------

/// Request-scoped image analyzer and aggregate-limit tracker.
#[derive(Debug, Default)]
pub struct Batch {
    /// Number of sources analyzed or rejected at the count boundary.
    count: usize,
    /// Decoded pixels accepted so far.
    decoded_pixels: u64,
}

impl Batch {
    /// Decode and measure one inline source under per-image and aggregate bounds.
    ///
    /// # Errors
    ///
    /// Returns a typed decoding or resource-limit failure.
    fn analyze_inline(
        &mut self,
        number: usize,
        declared: Option<MediaType>,
        encoded: &str,
    ) -> Result<Analysis, Error> {
        // Reject encoded payloads before allocating their decoded bytes.
        if encoded.len() > LIMIT_ENCODED_BYTES {
            return Err(Error::EncodedTooLarge {
                actual: encoded.len(),
                limit: LIMIT_ENCODED_BYTES,
            });
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|source| Error::InvalidBase64 {
                detail: source.to_string(),
            })?;
        let format = decode_format(&bytes)?;
        decode_validate_declared_format(declared, format)?;

        // Read headers before allocating the complete decoded frame.
        let reader = ImageReader::with_format(Cursor::new(&bytes), format);
        let (width, height) = reader
            .into_dimensions()
            .map_err(|source| Error::MalformedImage {
                detail: source.to_string(),
            })?;
        let pixels = decode_validate_dimensions(width, height)?;

        let total = self.decoded_pixels + pixels;

        // Reject aggregate decoded work before allocating the full frame.
        if total > LIMIT_REQUEST_PIXELS {
            return Err(Error::TotalAreaTooLarge {
                actual: total,
                limit: LIMIT_REQUEST_PIXELS,
            });
        }

        // PNG and JPEG expose a single decoded frame through this entry point.
        let image = image::load_from_memory_with_format(&bytes, format).map_err(|source| {
            Error::MalformedImage {
                detail: source.to_string(),
            }
        })?;
        self.decoded_pixels = total;
        Ok(Analysis::from_pixels(number, width, height, &image))
    }

    /// Analyze the next source and assign its one-based request number.
    ///
    /// # Errors
    ///
    /// Returns a typed invalid-input or fixed-limit diagnostic.
    pub fn analyze(&mut self, source: &Source) -> Result<Analysis, Error> {
        let number = self.count + 1;

        // Bound source count before retaining the next image number.
        if number > LIMIT_IMAGES {
            return Err(Error::TooManyImages {
                actual: number,
                limit: LIMIT_IMAGES,
            });
        }
        self.count = number;

        match &source.0 {
            SourceKind::Opaque => Ok(Analysis(AnalysisKind::Opaque { number })),
            SourceKind::Inline { declared, encoded } => {
                self.analyze_inline(number, *declared, encoded)
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Decode: Validates format and dimensions before full-frame allocation.
// -----------------------------------------------------------------------------

/// Detect one enabled image format and reject every other signature.
///
/// # Errors
///
/// Returns a typed malformed or unsupported-format failure.
fn decode_format(bytes: &[u8]) -> Result<ImageFormat, Error> {
    match image::guess_format(bytes) {
        Ok(format @ (ImageFormat::Png | ImageFormat::Jpeg)) => Ok(format),
        Ok(_) => Err(Error::UnsupportedFormat),
        Err(_) => Err(Error::MalformedImage {
            detail: "unrecognized image header".to_owned(),
        }),
    }
}

/// Require a declared type to agree with detected bytes.
///
/// # Errors
///
/// Returns [`Error::MediaTypeMismatch`] when a declaration disagrees.
fn decode_validate_declared_format(
    declared: Option<MediaType>,
    actual: ImageFormat,
) -> Result<(), Error> {
    let actual_media_type = match actual {
        ImageFormat::Png => MediaType::Png,
        ImageFormat::Jpeg => MediaType::Jpeg,
        // This helper accepts only formats already admitted by `decode_format`.
        _ => return Err(Error::UnsupportedFormat),
    };
    if declared.is_none_or(|declared| declared == actual_media_type) {
        Ok(())
    } else {
        Err(Error::MediaTypeMismatch {
            declared: declared.map_or("", MediaType::as_str),
            actual: actual_media_type.as_str(),
        })
    }
}

/// Enforce decoded dimension and per-image area bounds.
///
/// # Errors
///
/// Returns the corresponding fixed-limit failure when either bound is crossed.
fn decode_validate_dimensions(width: u32, height: u32) -> Result<u64, Error> {
    // Reject either excessive dimension before multiplying decoded area.
    if width > LIMIT_DIMENSION || height > LIMIT_DIMENSION {
        return Err(Error::DimensionsTooLarge {
            width,
            height,
            limit: LIMIT_DIMENSION,
        });
    }
    let pixels = u64::from(width) * u64::from(height);

    // Bound per-image decode work independently of the aggregate request.
    if pixels > LIMIT_IMAGE_PIXELS {
        return Err(Error::AreaTooLarge {
            actual: pixels,
            limit: LIMIT_IMAGE_PIXELS,
        });
    }
    Ok(pixels)
}

// -----------------------------------------------------------------------------
// Pixel: Converts decoded samples into deterministic integer values.
// -----------------------------------------------------------------------------

/// Composite one RGBA pixel over white using integer nearest rounding.
fn pixel_composite_white(pixel: Rgba<u8>) -> [u8; 3] {
    let [red, green, blue, alpha] = pixel.0;
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    [red, green, blue].map(|channel| {
        let composited = (u32::from(channel) * alpha + 255 * inverse + 127) / 255;
        u8::try_from(composited).unwrap_or(u8::MAX)
    })
}

/// Compute integer BT.601 luma for one composited RGB sample.
fn pixel_luma([red, green, blue]: [u8; 3]) -> u8 {
    let value = (77 * u32::from(red) + 150 * u32::from(green) + 29 * u32::from(blue) + 128) / 256;
    u8::try_from(value).unwrap_or(u8::MAX)
}

// -----------------------------------------------------------------------------
// OtsuScore: Compares exact between-class variance values.
// -----------------------------------------------------------------------------

/// Exact rational Otsu score used without floating point.
#[derive(Clone, Copy)]
struct OtsuScore {
    /// Squared difference between weighted class means.
    numerator: u128,
    /// Product of background and foreground class weights.
    denominator: u128,
}

impl OtsuScore {
    /// Compare two exact rational scores.
    const fn is_better_than(self, other: Self) -> bool {
        self.numerator * other.denominator > other.numerator * self.denominator
    }
}

// -----------------------------------------------------------------------------
// Otsu: Selects and reports the lowest maximum-variance luma threshold.
// -----------------------------------------------------------------------------

/// Sum every luma bin weighted by its value.
fn otsu_weighted_sum(histogram: &[u64; 256]) -> u64 {
    let indexed = histogram.iter().enumerate();
    indexed
        .map(|(luma, frequency)| luma as u64 * frequency)
        .sum()
}

/// Compute Otsu's threshold with exact rational comparisons and low-tie wins.
fn otsu_threshold(histogram: &[u64; 256]) -> u8 {
    // Establish whole-histogram moments before scanning possible partitions.
    let count = histogram.iter().sum::<u64>();
    let total = otsu_weighted_sum(histogram);
    let mut background_count = 0_u64;
    let mut background_sum = 0_u64;
    let mut best_threshold = 0_u8;
    let mut best_score: Option<OtsuScore> = None;

    // Test every nonempty foreground/background partition in threshold order.
    for (threshold, frequency) in histogram.iter().copied().enumerate() {
        background_count += frequency;
        background_sum += threshold as u64 * frequency;
        let foreground_count = count - background_count;
        if background_count == 0 || foreground_count == 0 {
            continue;
        }

        // Compare this partition with the best exact rational score so far.
        let difference = i128::from(total * background_count) - i128::from(background_sum * count);
        let candidate = OtsuScore {
            numerator: difference.unsigned_abs().pow(2),
            denominator: u128::from(background_count) * u128::from(foreground_count),
        };
        let is_better = best_score.is_none_or(|best| candidate.is_better_than(best));

        // Retain the earlier threshold when exact scores tie.
        if !is_better {
            continue;
        }
        best_score = Some(candidate);
        best_threshold = u8::try_from(threshold).unwrap_or(u8::MAX);
    }
    best_threshold
}

/// Round the thresholded histogram fraction to the nearest whole percent.
fn otsu_foreground_percentage(histogram: &[u64; 256], threshold: u8) -> u64 {
    let foreground = histogram[..=usize::from(threshold)].iter().sum::<u64>();
    let total = histogram.iter().sum::<u64>();
    (foreground * 100 + total / 2) / total
}

// -----------------------------------------------------------------------------
// Tests: Pin decoding, classification, Otsu behavior, and fixed bounds.
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
#[expect(
    rlib::ad_hoc_conversions,
    reason = "the PNG helper is fixture construction rather than a domain conversion"
)]
mod tests {
    use std::io::Cursor;

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::*;

    /// Side length of one four-million-pixel aggregate fixture.
    const AGGREGATE_SIDE: u32 = 2000;

    /// Number of maximum-area fixtures that fill the aggregate allowance.
    const AGGREGATE_IMAGES: usize = 2;

    /// Encode a generated image through the same supported codec surface.
    fn encoded_source(image: &DynamicImage, format: ImageFormat, media_type: &str) -> Source {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        Source::typed_inline_base64(media_type, STANDARD.encode(bytes.into_inner())).unwrap()
    }

    /// Build a solid RGBA fixture with explicit dimensions.
    fn solid(width: u32, height: u32, pixel: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(pixel)))
    }

    /// Analyze one generated PNG fixture.
    fn analyze_png(image: &DynamicImage) -> Analysis {
        Batch::default()
            .analyze(&encoded_source(image, ImageFormat::Png, "image/png"))
            .unwrap()
    }

    #[test]
    fn classifies_solid_dark_and_light_images() {
        assert_eq!(
            analyze_png(&solid(1, 1, [0, 0, 0, 255])).to_string(),
            "[RETRO VISION 1: 1x1 SQUARE, DARK, NEUTRAL, LOW CONTRAST, 100% FOREGROUND]"
        );
        assert_eq!(
            analyze_png(&solid(1, 1, [255, 255, 255, 255])).to_string(),
            "[RETRO VISION 1: 1x1 SQUARE, LIGHT, NEUTRAL, LOW CONTRAST, 0% FOREGROUND]"
        );
    }

    #[test]
    fn classifies_warm_cool_and_neutral_images() {
        assert!(
            analyze_png(&solid(1, 1, [255, 0, 0, 255]))
                .to_string()
                .contains("WARM")
        );
        assert!(
            analyze_png(&solid(1, 1, [0, 0, 255, 255]))
                .to_string()
                .contains("COOL")
        );
        assert!(
            analyze_png(&solid(1, 1, [128, 128, 128, 255]))
                .to_string()
                .contains("NEUTRAL")
        );
    }

    #[test]
    fn classifies_every_aspect() {
        assert!(
            analyze_png(&solid(8, 10, [0, 0, 0, 255]))
                .to_string()
                .contains("PORTRAIT")
        );
        assert!(
            analyze_png(&solid(10, 10, [0, 0, 0, 255]))
                .to_string()
                .contains("SQUARE")
        );
        assert!(
            analyze_png(&solid(12, 10, [0, 0, 0, 255]))
                .to_string()
                .contains("LANDSCAPE")
        );
    }

    #[test]
    fn classifies_every_contrast() {
        let medium = DynamicImage::ImageRgba8(RgbaImage::from_fn(64, 64, |x, _| {
            Rgba(if x < 32 {
                [0, 0, 0, 255]
            } else {
                [128, 128, 128, 255]
            })
        }));
        let high = DynamicImage::ImageRgba8(RgbaImage::from_fn(64, 64, |x, _| {
            Rgba(if x < 32 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            })
        }));
        assert!(
            analyze_png(&solid(1, 1, [64, 64, 64, 255]))
                .to_string()
                .contains("LOW CONTRAST")
        );
        assert!(analyze_png(&medium).to_string().contains("MEDIUM CONTRAST"));
        assert!(analyze_png(&high).to_string().contains("HIGH CONTRAST"));
    }

    #[test]
    fn composites_alpha_over_white() {
        assert_eq!(
            analyze_png(&solid(1, 1, [255, 0, 0, 0])).to_string(),
            "[RETRO VISION 1: 1x1 SQUARE, LIGHT, NEUTRAL, LOW CONTRAST, 0% FOREGROUND]"
        );
    }

    #[test]
    fn decodes_jpeg() {
        let source = encoded_source(
            &solid(3, 2, [230, 20, 10, 255]),
            ImageFormat::Jpeg,
            "image/jpeg",
        );
        let analysis = Batch::default().analyze(&source).unwrap().to_string();
        assert!(analysis.contains("3x2 LANDSCAPE"));
        assert!(analysis.contains("WARM"));
    }

    #[test]
    fn chooses_the_lowest_otsu_threshold_on_ties() {
        let mut histogram = [0; 256];
        histogram[10] = 1;
        histogram[20] = 1;
        assert_eq!(otsu_threshold(&histogram), 10);
    }

    #[test]
    fn rounds_foreground_to_the_nearest_percent() {
        let mut rounded_up = [0; 256];
        rounded_up[0] = 21;
        rounded_up[1] = 19;
        assert_eq!(otsu_foreground_percentage(&rounded_up, 0), 53);

        let mut exact = [0; 256];
        exact[0] = 20;
        exact[1] = 20;
        assert_eq!(otsu_foreground_percentage(&exact, 0), 50);
    }

    #[test]
    fn repeats_analysis_exactly() {
        let source = encoded_source(
            &solid(7, 9, [20, 40, 200, 255]),
            ImageFormat::Png,
            "image/png",
        );
        let first = Batch::default().analyze(&source).unwrap();
        let second = Batch::default().analyze(&source).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn leaves_http_and_provider_sources_opaque() {
        let sources = [
            Source::url("https://127.0.0.1:1/unreachable.png").unwrap(),
            Source::provider_reference("file-never-opened", None).unwrap(),
        ];
        let mut batch = Batch::default();
        assert_eq!(
            batch.analyze(&sources[0]).unwrap().to_string(),
            "[RETRO VISION 1: OPAQUE IMAGE NOT INSPECTED]"
        );
        assert_eq!(
            batch.analyze(&sources[1]).unwrap().to_string(),
            "[RETRO VISION 2: OPAQUE IMAGE NOT INSPECTED]"
        );
    }

    #[test]
    fn rejects_malformed_base64_and_image_bytes() {
        let invalid_base64 = Source::inline_base64("%%%".to_owned()).unwrap();
        assert!(matches!(
            Batch::default().analyze(&invalid_base64),
            Err(Error::InvalidBase64 { .. })
        ));

        let malformed = Source::inline_base64(STANDARD.encode(b"not an image")).unwrap();
        assert!(matches!(
            Batch::default().analyze(&malformed),
            Err(Error::MalformedImage { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_and_incomplete_sources() {
        assert!(matches!(
            Source::url("file:///tmp/image.png"),
            Err(Error::UnsupportedUrlScheme)
        ));
        assert!(matches!(
            Source::typed_inline_base64("image/gif", "R0lGODlh".to_owned()),
            Err(Error::UnsupportedMediaType { .. })
        ));
        assert!(matches!(
            Source::url_or_reference(None, None),
            Err(Error::MissingSource)
        ));
        assert!(matches!(
            Source::url_or_reference(
                Some("https://example.invalid/a".to_owned()),
                Some("file".to_owned())
            ),
            Err(Error::ConflictingSources)
        ));
    }

    #[test]
    fn rejects_mime_mismatch_and_unsupported_format() {
        let mismatch = encoded_source(&solid(1, 1, [0, 0, 0, 255]), ImageFormat::Png, "image/jpeg");
        assert!(matches!(
            Batch::default().analyze(&mismatch),
            Err(Error::MediaTypeMismatch { .. })
        ));

        let unsupported = Source::inline_base64(STANDARD.encode(b"GIF89a payload")).unwrap();
        assert!(matches!(
            Batch::default().analyze(&unsupported),
            Err(Error::UnsupportedFormat)
        ));
    }

    #[test]
    fn enforces_image_count_and_encoded_size() {
        let opaque = Source::provider_reference("file", None).unwrap();
        let mut batch = Batch::default();
        for _ in 0..LIMIT_IMAGES {
            batch.analyze(&opaque).unwrap();
        }
        assert!(matches!(
            batch.analyze(&opaque),
            Err(Error::TooManyImages { .. })
        ));

        let oversized = Source::inline_base64("A".repeat(LIMIT_ENCODED_BYTES + 1)).unwrap();
        assert!(matches!(
            Batch::default().analyze(&oversized),
            Err(Error::EncodedTooLarge { .. })
        ));
    }

    #[test]
    fn enforces_dimensions_and_per_image_area() {
        let wide = encoded_source(
            &solid(LIMIT_DIMENSION + 1, 1, [0, 0, 0, 255]),
            ImageFormat::Png,
            "image/png",
        );
        assert!(matches!(
            Batch::default().analyze(&wide),
            Err(Error::DimensionsTooLarge { .. })
        ));

        let large = encoded_source(
            &solid(2001, 2000, [0, 0, 0, 255]),
            ImageFormat::Png,
            "image/png",
        );
        assert!(matches!(
            Batch::default().analyze(&large),
            Err(Error::AreaTooLarge { .. })
        ));
    }

    #[test]
    fn enforces_aggregate_decoded_area() {
        let maximum = encoded_source(
            &solid(AGGREGATE_SIDE, AGGREGATE_SIDE, [0, 0, 0, u8::MAX]),
            ImageFormat::Png,
            "image/png",
        );
        let extra = encoded_source(&solid(1, 1, [0, 0, 0, 255]), ImageFormat::Png, "image/png");
        let mut batch = Batch::default();
        for _ in 0..AGGREGATE_IMAGES {
            batch.analyze(&maximum).unwrap();
        }
        assert!(matches!(
            batch.analyze(&extra),
            Err(Error::TotalAreaTooLarge { .. })
        ));
    }
}
