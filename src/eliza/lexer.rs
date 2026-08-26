//! Lexer for the compact S-expression script language.

use super::syntax::Span;

// -----------------------------------------------------------------------------
// TokenKind: Recognizes the compact S-expression vocabulary.
// -----------------------------------------------------------------------------

/// Enumerates the supported `TokenKind` cases.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum TokenKind {
    /// Represents the `OpenParen` case.
    OpenParen,
    /// Represents the `CloseParen` case.
    CloseParen,
    /// Stores the wrapped value owned by this declaration.
    Atom(
        /// Raw non-parenthesis token text.
        String,
    ),
}

// -----------------------------------------------------------------------------
// Token: Retains one lexeme and its source position.
// -----------------------------------------------------------------------------

/// Represents `Token` state within this module.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct Token {
    /// Raw script token before ELIZA-specific meaning is attached.
    pub(super) kind: TokenKind,
    /// Start position used for diagnostics after later lowering failures.
    pub(super) span: Span,
}

// -----------------------------------------------------------------------------
// LexedScript: Owns the token stream and EOF position.
// -----------------------------------------------------------------------------

/// Represents `LexedScript` state within this module.
#[derive(Debug, Clone)]
pub(super) struct LexedScript {
    /// Complete token stream without comments or whitespace.
    pub(super) tokens: Vec<Token>,
    /// Final position used when EOF appears inside an open list.
    pub(super) eof: Span,
}

impl From<&str> for LexedScript {
    fn from(input: &str) -> Self {
        let mut lexer = Lexer::for_script(input);
        let mut tokens = Vec::new();

        // Keep token positions while discarding comments and whitespace.
        while let Some(token) = lexer.next_token() {
            tokens.push(token);
        }
        Self {
            tokens,
            eof: Span {
                line: lexer.line,
                column: lexer.column,
            },
        }
    }
}

// -----------------------------------------------------------------------------
// Lexer: Scans source into positioned tokens.
// -----------------------------------------------------------------------------

/// Represents `Lexer` state within this module.
struct Lexer<'a> {
    /// Stores the input value owned by this contract.
    input: &'a str,
    /// Stores the offset value owned by this contract.
    offset: usize,
    /// Stores the line value owned by this contract.
    line: usize,
    /// Stores the column value owned by this contract.
    column: usize,
}

impl<'a> Lexer<'a> {
    /// Create a lexer over one script source.
    fn for_script(input: &'a str) -> Self {
        Self {
            input,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    /// Performs the peek operation for this abstraction.
    fn peek(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    /// Performs the bump operation for this abstraction.
    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    /// Consume one source comment through its terminating newline.
    fn skip_comment(&mut self) {
        while let Some(character) = self.peek() {
            self.bump();
            if character == '\n' {
                break;
            }
        }
    }

    /// Performs the skip whitespace and comments operation for this abstraction.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(';') => {
                    // Script comments run to the end of the current line.
                    self.skip_comment();
                }
                Some(character) if character.is_whitespace() => {
                    self.bump();
                }
                _ => break,
            }
        }
    }

    /// Performs the atom operation for this abstraction.
    fn read_atom_token(&mut self) -> Token {
        let span = Span {
            line: self.line,
            column: self.column,
        };
        let mut atom = String::new();

        while let Some(character) = self.peek() {
            if character.is_whitespace() || character == '(' || character == ')' || character == ';'
            {
                break;
            }
            atom.push(character);
            self.bump();
        }

        Token {
            kind: TokenKind::Atom(atom),
            span,
        }
    }

    /// Performs the next token operation for this abstraction.
    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace_and_comments();
        let span = Span {
            line: self.line,
            column: self.column,
        };
        let character = self.peek()?;

        match character {
            '(' => {
                self.bump();
                Some(Token {
                    kind: TokenKind::OpenParen,
                    span,
                })
            }
            ')' => {
                self.bump();
                Some(Token {
                    kind: TokenKind::CloseParen,
                    span,
                })
            }
            _ => Some(self.read_atom_token()),
        }
    }
}

// -----------------------------------------------------------------------------
