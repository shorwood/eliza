//! IBM 7094 BCD words and the original SLIP hash.

use thiserror::Error;

// -----------------------------------------------------------------------------
// BcdWord: Encodes IBM 7094 words and the original SLIP hash.
// -----------------------------------------------------------------------------

/// Number of BCD characters stored in one machine word.
const BCD_WORD_CHARACTERS: usize = 6;

/// Largest argument accepted by the historical SLIP hash.
const BCD_WORD_HASH_MAX_BITS: u8 = 15;

/// Low 36 bits retained by a BCD word.
const BCD_WORD_MASK: u64 = (1_u64 << 36) - 1;

/// IBM 7094 six-bit character repertoire indexed by code point.
const BCD_WORD_REPERTOIRE: [Option<char>; 64] = [
    Some('0'),
    Some('1'),
    Some('2'),
    Some('3'),
    Some('4'),
    Some('5'),
    Some('6'),
    Some('7'),
    Some('8'),
    Some('9'),
    None,
    Some('='),
    Some('\''),
    None,
    None,
    None,
    Some('+'),
    Some('A'),
    Some('B'),
    Some('C'),
    Some('D'),
    Some('E'),
    Some('F'),
    Some('G'),
    Some('H'),
    Some('I'),
    None,
    Some('.'),
    Some(')'),
    None,
    None,
    None,
    Some('-'),
    Some('J'),
    Some('K'),
    Some('L'),
    Some('M'),
    Some('N'),
    Some('O'),
    Some('P'),
    Some('Q'),
    Some('R'),
    None,
    Some('$'),
    Some('*'),
    None,
    None,
    None,
    Some(' '),
    Some('/'),
    Some('S'),
    Some('T'),
    Some('U'),
    Some('V'),
    Some('W'),
    Some('X'),
    Some('Y'),
    Some('Z'),
    None,
    Some(','),
    Some('('),
    None,
    None,
    None,
];

/// One 36-bit word containing six BCD characters.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BcdWord(
    /// Raw six-bit character codes packed into a 36-bit word.
    u64,
);

impl BcdWord {
    /// Encode up to six BCD characters, left justified and space padded.
    ///
    /// # Errors
    ///
    /// Returns an error for longer strings or characters outside the 7094
    /// repertoire.
    pub fn encode(text: &str) -> Result<Self, BcdError> {
        // One machine word cannot preserve additional characters.
        if text.chars().count() > BCD_WORD_CHARACTERS {
            return Err(BcdError::TooLong);
        }
        let mut raw = 0_u64;
        let uppercase = text.chars().map(|character| character.to_ascii_uppercase());
        let padded = uppercase.chain(std::iter::repeat(' '));
        for character in padded.take(BCD_WORD_CHARACTERS) {
            let code = BCD_WORD_REPERTOIRE
                .iter()
                .position(|candidate| *candidate == Some(character))
                .ok_or(BcdError::Unsupported(character))?;
            let code = u8::try_from(code).map_err(|_| BcdError::Unsupported(character))?;
            raw = (raw << BCD_WORD_CHARACTERS) | u64::from(code);
        }
        Ok(Self(raw))
    }

    /// Encode the last six-character storage chunk of a word.
    ///
    /// # Errors
    ///
    /// Returns an error for a character outside the 7094 repertoire.
    pub fn last_chunk(text: &str) -> Result<Self, BcdError> {
        let characters = text.chars().collect::<Vec<_>>();
        let start = characters.len().saturating_sub(1) / BCD_WORD_CHARACTERS * BCD_WORD_CHARACTERS;
        let chunk = characters[start..].iter().collect::<String>();
        Self::encode(&chunk)
    }

    /// Construct from the low 36 bits of a machine word.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw & BCD_WORD_MASK)
    }

    /// Decode six characters, including trailing spaces.
    #[must_use]
    pub fn decode(self) -> String {
        (0..BCD_WORD_CHARACTERS)
            .map(|index| {
                let shift = (BCD_WORD_CHARACTERS - 1 - index) * BCD_WORD_CHARACTERS;
                let code = ((self.0 >> shift) & 0x3f) as usize;
                BCD_WORD_REPERTOIRE[code].unwrap_or('?')
            })
            .collect()
    }

    /// Return the raw 36-bit word.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Reproduce SLIP's von Neumann middle-square hash.
    ///
    /// # Panics
    ///
    /// Panics when `bits` exceeds the historical 15-bit argument range.
    #[must_use]
    pub fn hash(self, bits: u8) -> u16 {
        assert!(
            bits <= BCD_WORD_HASH_MAX_BITS,
            "SLIP HASH accepts at most 15 bits"
        );
        let magnitude = u128::from(self.raw() & ((1_u64 << 35) - 1));
        let square = magnitude * magnitude;
        let shifted = square >> (35 - usize::from(bits / 2));
        let mask = (1_u128 << bits) - 1;
        u16::try_from(shifted & mask).expect("at most 15 hash bits fit in u16")
    }
}

// -----------------------------------------------------------------------------
// BcdError: Reports unsupported words and characters.
// -----------------------------------------------------------------------------

/// BCD conversion failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BcdError {
    /// More than one machine word was supplied to [`BcdWord::encode`].
    #[error("a BCD word contains at most six characters")]
    TooLong,
    /// Character is outside the historical 64-code repertoire.
    #[error("{0:?} is not an IBM 7094 BCD character")]
    Unsupported(
        /// Unsupported source character.
        char,
    ),
}

// -----------------------------------------------------------------------------
// Tests: Verify historical encodings, chunks, and hash examples.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::BcdWord;

    /// # Panics
    ///
    /// Panics when encoding or an assertion fails.
    #[test]
    fn encodes_historical_words() {
        assert_eq!(BcdWord::encode("ALWAYS").unwrap().raw(), 0o214_366_217_062);
        assert_eq!(BcdWord::encode("HERE").unwrap().raw(), 0o302_551_256_060);
        assert_eq!(BcdWord::encode("HERE").unwrap().decode(), "HERE  ");
    }

    /// # Panics
    ///
    /// Panics when encoding or an assertion fails.
    #[test]
    fn hashes_historical_examples() {
        assert_eq!(BcdWord::encode("ALWAYS").unwrap().hash(7), 14);
        assert_eq!(BcdWord::encode("HERE").unwrap().hash(2), 3);
        assert_eq!(BcdWord::encode("KIDS").unwrap().hash(2), 1);
        assert_eq!(BcdWord::encode("TIME").unwrap().hash(2), 0);
    }

    /// # Panics
    ///
    /// Panics when encoding or the assertion fails.
    #[test]
    fn uses_the_final_storage_chunk() {
        assert_eq!(
            BcdWord::last_chunk("INVENTED").unwrap(),
            BcdWord::encode("ED").unwrap()
        );
    }
}
