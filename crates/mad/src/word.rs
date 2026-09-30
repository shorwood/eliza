//! IBM 7094 words used by the interpreter.

use std::fmt;

// -----------------------------------------------------------------------------
// Word: Preserves bounded IBM 7094 values and their bit layout.
// -----------------------------------------------------------------------------

/// Mask covering one 36-bit machine word.
pub const WORD_MASK: u64 = (1_u64 << 36) - 1;

/// Sign bit in a 36-bit machine word.
const WORD_SIGN_BIT: u64 = 1_u64 << 35;

/// Mask covering the 35 magnitude bits.
const WORD_MAGNITUDE_MASK: u64 = WORD_SIGN_BIT - 1;

/// One IBM 7094 sign-magnitude word.
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub struct Word(
    /// Raw value, always masked to 36 bits by the constructors.
    u64,
);

impl Word {
    /// The zero word.
    pub const ZERO: Self = Self(0);

    /// Construct a word from its low 36 raw bits.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw & WORD_MASK)
    }

    /// Encode a signed integer using 7094 sign-magnitude representation.
    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        let magnitude = value.unsigned_abs() & WORD_MAGNITUDE_MASK;
        let sign = u64::from(value.is_negative()) * WORD_SIGN_BIT;
        Self(sign | magnitude)
    }

    /// Return the raw 36-bit representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Decode this word as a sign-magnitude integer.
    #[must_use]
    pub fn to_i64(self) -> i64 {
        let magnitude = (self.0 & WORD_MAGNITUDE_MASK).cast_signed();
        if self.0 & WORD_SIGN_BIT == 0 {
            magnitude
        } else {
            -magnitude
        }
    }

    /// MAD truthiness: either representation of zero is false.
    #[must_use]
    pub const fn is_true(self) -> bool {
        self.0 & WORD_MAGNITUDE_MASK != 0
    }
}

impl fmt::Debug for Word {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("Word");
        debug.field("raw", &format_args!("{:012o}", self.raw()));
        debug.field("integer", &self.to_i64());
        debug.finish()
    }
}

impl From<i64> for Word {
    fn from(value: i64) -> Self {
        Self::from_i64(value)
    }
}

// -----------------------------------------------------------------------------
// Tests: Verify raw bounds and sign-magnitude conversion.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{WORD_MASK, Word};

    /// # Panics
    ///
    /// Panics when a signed value does not round-trip.
    #[test]
    fn sign_magnitude_round_trips() {
        for value in [-123, -1, 0, 1, 123] {
            assert_eq!(Word::from_i64(value).to_i64(), value);
        }
    }

    /// # Panics
    ///
    /// Panics when raw construction exceeds the machine-word mask.
    #[test]
    fn raw_words_are_bounded() {
        assert_eq!(Word::from_raw(u64::MAX).raw(), WORD_MASK);
    }
}
