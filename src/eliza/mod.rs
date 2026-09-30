//! Provider input adapter for the MAD-SLIP ELIZA engine.

use std::sync::OnceLock;

use eliza_mad::engine::{Doctor, Error};

// -----------------------------------------------------------------------------
// Doctor: Compiles and shares the historical program.
// -----------------------------------------------------------------------------

/// Compiled historical program shared by fresh request sessions.
static DOCTOR: OnceLock<Doctor> = OnceLock::new();

/// Return the compiled 1966 DOCTOR program.
///
/// # Errors
///
/// Returns a source, link, or script error when a bundled artifact is invalid.
pub(crate) fn doctor() -> Result<&'static Doctor, Error> {
    // Reuse the compiled program after the first successful validation.
    if let Some(doctor) = DOCTOR.get() {
        return Ok(doctor);
    }

    // A concurrent initializer may win; both validated values are equivalent.
    let doctor = Doctor::compile()?;
    Ok(DOCTOR.get_or_init(|| doctor))
}

// -----------------------------------------------------------------------------
// InputRecord: Adapts provider text to the historical terminal.
// -----------------------------------------------------------------------------

/// Maximum characters in one IBM 7094 terminal record.
const INPUT_RECORD_COLUMNS: usize = 72;

/// Adapt provider text to one uppercase historical terminal record.
#[must_use]
pub(crate) fn input_record(input: &str) -> String {
    let normalized = input
        .chars()
        .map(|character| match character {
            '\u{2018}' | '\u{2019}' => '\'',
            ',' | '.' | '\'' => character,
            value if value.is_ascii_alphanumeric() => value.to_ascii_uppercase(),
            _ => ' ',
        })
        .collect::<String>();

    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(INPUT_RECORD_COLUMNS)
        .collect()
}

// -----------------------------------------------------------------------------
// Tests: Pin provider-to-terminal input adaptation.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{INPUT_RECORD_COLUMNS, input_record};

    /// Provider text becomes a bounded uppercase historical record.
    ///
    /// # Panics
    ///
    /// Panics when normalization or the record bound changes.
    #[test]
    fn adapts_provider_text() {
        assert_eq!(input_record("I’m worried—really!"), "I'M WORRIED REALLY");
        assert_eq!(input_record(&"a".repeat(73)).len(), INPUT_RECORD_COLUMNS);
    }
}
