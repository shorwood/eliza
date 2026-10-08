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
pub(super) fn doctor() -> Result<&'static Doctor, Error> {
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
pub(super) fn input_record(input: &str) -> String {
    let mut record = String::with_capacity(input.len().min(INPUT_RECORD_COLUMNS));
    let mut pending_space = false;
    for character in input.chars() {
        // Map provider characters to the terminal's ASCII alphabet.
        let character = match character {
            '\u{2018}' | '\u{2019}' => '\'',
            ',' | '.' | '\'' => character,
            value if value.is_ascii_alphanumeric() => value.to_ascii_uppercase(),
            _ => ' ',
        };

        // Defer separators until another retained character follows.
        if character == ' ' {
            pending_space = !record.is_empty();
            continue;
        }
        if pending_space {
            record.push(' ');
        }

        // Truncation can retain the separator at column 72.
        if record.len() == INPUT_RECORD_COLUMNS {
            break;
        }
        record.push(character);
        if record.len() == INPUT_RECORD_COLUMNS {
            break;
        }
        pending_space = false;
    }
    record
}
