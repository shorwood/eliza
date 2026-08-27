//! Recursive parser from positioned tokens to S-expressions.

use super::lexer::{LexedScript, Token, TokenKind};
use super::syntax::{Sexp, SexpKind, Span};
use crate::errors::AppError;

// -----------------------------------------------------------------------------
// Parser: Builds expressions from positioned tokens.
// -----------------------------------------------------------------------------

/// Cursor that turns positioned tokens into nested S-expressions.
pub(super) struct Parser {
    /// Token stream produced by the lexer.
    tokens: Vec<Token>,
    /// Current parser position into `tokens`.
    cursor: usize,
    /// Span returned when a list reaches EOF before a close paren.
    eof: Span,
}

impl Parser {
    /// Return the token at the current parser position.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    /// Parse one atom or list at the current cursor.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the cursor is at EOF or an unexpected close
    /// paren.
    fn parse_one(&mut self) -> Result<Sexp, AppError> {
        let token = self.peek().ok_or(AppError::ScriptUnexpectedEnd {
            line: self.eof.line,
            column: self.eof.column,
        })?;

        // Dispatch the positioned token into its expression representation.
        match &token.kind {
            TokenKind::OpenParen => self.parse_list(),
            TokenKind::CloseParen => Err(AppError::ScriptUnexpectedClose {
                line: token.span.line,
                column: token.span.column,
            }),
            TokenKind::Atom(atom) => {
                let atom = atom.clone();
                let span = token.span;
                self.cursor += 1;
                Ok(Sexp {
                    kind: SexpKind::Atom(atom),
                    span,
                })
            }
        }
    }

    /// Parse one list starting at an open paren.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptUnexpectedEnd`] when EOF appears before the
    /// closing paren.
    fn parse_list(&mut self) -> Result<Sexp, AppError> {
        let open = self.next().ok_or(AppError::ScriptUnexpectedEnd {
            line: self.eof.line,
            column: self.eof.column,
        })?;
        let mut items = Vec::new();

        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::CloseParen,
                    ..
                }) => {
                    self.cursor += 1;
                    return Ok(Sexp {
                        kind: SexpKind::List(items),
                        span: open.span,
                    });
                }
                Some(_) => items.push(self.parse_one()?),
                None => {
                    return Err(AppError::ScriptUnexpectedEnd {
                        line: self.eof.line,
                        column: self.eof.column,
                    });
                }
            }
        }
    }

    /// Report whether every lexed token has been consumed.
    fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    /// Parse every token from the lexer.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] from the first malformed S-expression.
    fn parse_all(mut self) -> Result<Vec<Sexp>, AppError> {
        let mut sexps = Vec::new();

        // Lower each independent top-level source form in sequence.
        while !self.is_eof() {
            sexps.push(self.parse_one()?);
        }
        Ok(sexps)
    }

    /// Parse a full script source into S-expressions.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when parentheses are unbalanced or the parser sees
    /// a close paren without a matching open paren.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eliza::eliza::parser::Parser;
    ///
    /// let expressions = Parser::parse("(RULE (0 TEST 0))").unwrap();
    /// let outer = expressions[0].list().unwrap();
    ///
    /// // Parsing preserves nesting instead of flattening the rule.
    /// assert_eq!(outer.atom_at(0), Some("RULE"));
    /// assert_eq!(outer.get(1).unwrap().list().unwrap().atom_at(1), Some("TEST"));
    ///
    /// let error = Parser::parse("(RULE").unwrap_err();
    ///
    /// // EOF diagnostics point just after the final source character.
    /// assert_eq!(error.to_string(), "unexpected end of script at 1:6");
    /// ```
    pub(super) fn parse(input: &str) -> Result<Vec<Sexp>, AppError> {
        let lexed = LexedScript::from(input);
        Self {
            tokens: lexed.tokens,
            cursor: 0,
            eof: lexed.eof,
        }
        .parse_all()
    }
}

impl Iterator for Parser {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.tokens.get(self.cursor).cloned();
        if token.is_some() {
            self.cursor += 1;
        }
        token
    }
}
