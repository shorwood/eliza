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

    /// Original whole-input adapter retained only as a compatibility oracle.
    fn original_record(input: &str) -> String {
        let normalized = input
            .chars()
            .map(|character| match character {
                '\u{2018}' | '\u{2019}' => '\'',
                ',' | '.' | '\'' => character,
                value if value.is_ascii_alphanumeric() => value.to_ascii_uppercase(),
                _ => ' ',
            })
            .collect::<String>();
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        let single_spaced = words.join(" ");
        single_spaced.chars().take(INPUT_RECORD_COLUMNS).collect()
    }

    /// Pin leading/trailing separators and both ways to fill the last column.
    ///
    /// # Panics
    /// Panics when exact normalization or truncation changes.
    #[test]
    fn preserves_record_boundaries() {
        for (input, expected) in [
            ("", ""),
            (" \t\n—😀é\u{301}", ""),
            (" —hello\t\nworld! ", "HELLO WORLD"),
            ("a, b. ‘c’ 'd'", "A, B. 'C' 'D'"),
        ] {
            assert_eq!(input_record(input), expected);
        }
        for length in [71, 72, 73] {
            assert_eq!(
                input_record(&"a".repeat(length)),
                "A".repeat(length.min(72))
            );
        }
        let prefix = "a".repeat(71);
        assert_eq!(
            input_record(&format!("{prefix} — b")),
            format!("{} ", "A".repeat(71))
        );
        assert_eq!(input_record(&format!("{prefix} — ")), "A".repeat(71));
        assert_eq!(
            input_record(&format!("{}hello", "\u{2003}".repeat(4096))),
            "HELLO"
        );
        assert_eq!(
            input_record(&format!("{}{}", "a".repeat(72), "—".repeat(4096))),
            "A".repeat(72)
        );
    }

    /// Compare mixed mappings and truncation against the original expression.
    ///
    /// # Panics
    /// Panics when any deterministic corpus entry changes its record.
    #[test]
    fn matches_original_adapter() {
        let ascii = (0_u8..=127).map(char::from).collect::<String>();
        assert_eq!(input_record(&ascii), original_record(&ascii));
        let fragments = ["aZ09", ",.'", "‘’", " \t\r\n", "é😀—\u{301}\u{2003}"];
        let pairs = fragments
            .into_iter()
            .flat_map(|left| fragments.map(|right| (left, right)));
        for (left, right) in pairs {
            for length in [0, 1, 2, 70, 71, 72, 73, 256] {
                let input = format!("{}{right}{left}", left.repeat(length));
                assert_eq!(input_record(&input), original_record(&input), "{input:?}");
            }
        }
    }
}
