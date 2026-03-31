//! UNIQL Lexer — Tokenizer
//!
//! Converts a UNIQL query string into a stream of tokens.
//! Handwritten for maximum control over error messages and performance.

use std::fmt;
use thiserror::Error;

// ─── Token Types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum TokenKind {
    // Keywords
    From,
    Where,
    Within,
    Compute,
    Show,
    Correlate,
    On,
    GroupBy,
    Having,
    As,
    And,
    Or,
    Not,
    In,
    By,
    Last,
    To,
    Today,
    ThisWeek,
    Parse,
    Filter,
    Define,
    Use,
    Explain,
    Validate,
    Fill,
    Native,

    // Identifiers & Literals
    Ident(String),       // service, host, __name__
    StringLit(String),   // "nginx", 'api'
    NumberLit(f64),      // 42, 3.14
    DurationLit(String), // 5m, 1h, 24h, 500ms, 30s

    // Operators
    Eq,           // =
    Neq,          // !=
    Gt,           // >
    Lt,           // <
    Gte,          // >=
    Lte,          // <=
    RegexMatch,   // =~
    RegexNoMatch, // !~
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Percent,      // %

    // Delimiters
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    Comma,     // ,
    Dot,       // .
    Colon,     // :
    Pipe,      // |
    PipeArrow, // |>

    // String match operators (contextual keywords)
    Contains,
    StartsWith,
    Matches,

    // Special
    Eof,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} @ {}..{}",
            self.kind, self.span.start, self.span.end
        )
    }
}

// ─── Errors ────────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum LexError {
    #[error("Unexpected character '{ch}' at position {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    #[error("Unterminated string starting at position {pos}")]
    UnterminatedString { pos: usize },

    #[error("Invalid number '{text}' at position {pos}")]
    InvalidNumber { text: String, pos: usize },

    #[error(
        "Invalid duration '{text}' at position {pos}. Expected format like 5m, 1h, 30s, 500ms"
    )]
    InvalidDuration { text: String, pos: usize },
}

// ─── Lexer ─────────────────────────────────────────────────────────────────────

struct Lexer {
    input: Vec<char>,
    pos: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        self.pos += 1;
        ch
    }

    fn emit(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
        });
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_string(&mut self, quote: char) -> Result<String, LexError> {
        let start = self.pos - 1; // quote already consumed
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(ch) if ch == quote => return Ok(s),
                Some('\\') => {
                    // Escape sequences
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('\\') => s.push('\\'),
                        Some(c) if c == quote => s.push(c),
                        _ => s.push('\\'),
                    }
                }
                Some(ch) => s.push(ch),
                None => return Err(LexError::UnterminatedString { pos: start }),
            }
        }
    }

    fn read_number(&mut self, first: char) -> Result<(f64, String), LexError> {
        let start = self.pos - 1;
        let mut text = String::new();
        text.push(first);

        // Read digits and dots
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' {
                text.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for duration suffix
        let mut suffix = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphabetic() {
                suffix.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if !suffix.is_empty() {
            // It's a duration literal
            let full = format!("{}{}", text, suffix);
            match suffix.as_str() {
                "ms" | "s" | "m" | "h" | "d" | "w" | "y" => {
                    return Ok((0.0, full)); // duration marker
                }
                _ => {
                    return Err(LexError::InvalidDuration {
                        text: full,
                        pos: start,
                    });
                }
            }
        }

        let value = text.parse::<f64>().map_err(|_| LexError::InvalidNumber {
            text: text.clone(),
            pos: start,
        })?;

        Ok((value, String::new()))
    }

    fn read_ident(&mut self, first: char) -> String {
        let mut ident = String::new();
        ident.push(first);

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        ident
    }

    fn tokenize(&mut self) -> Result<(), LexError> {
        while self.pos < self.input.len() {
            self.skip_whitespace();

            if self.pos >= self.input.len() {
                break;
            }

            let start = self.pos;
            let ch = self.advance().unwrap();

            match ch {
                // Comments
                '-' if self.peek() == Some('-') => {
                    self.advance();
                    self.skip_line_comment();
                }

                // Strings
                '"' | '\'' => {
                    let s = self.read_string(ch)?;
                    self.emit(TokenKind::StringLit(s), start);
                }

                // Numbers
                '0'..='9' => {
                    let (value, duration) = self.read_number(ch)?;
                    if !duration.is_empty() {
                        self.emit(TokenKind::DurationLit(duration), start);
                    } else {
                        self.emit(TokenKind::NumberLit(value), start);
                    }
                }

                // Operators
                '=' if self.peek() == Some('~') => {
                    self.advance();
                    self.emit(TokenKind::RegexMatch, start);
                }
                '=' => self.emit(TokenKind::Eq, start),

                '!' if self.peek() == Some('=') => {
                    self.advance();
                    self.emit(TokenKind::Neq, start);
                }
                '!' if self.peek() == Some('~') => {
                    self.advance();
                    self.emit(TokenKind::RegexNoMatch, start);
                }

                '>' if self.peek() == Some('=') => {
                    self.advance();
                    self.emit(TokenKind::Gte, start);
                }
                '>' => self.emit(TokenKind::Gt, start),

                '<' if self.peek() == Some('=') => {
                    self.advance();
                    self.emit(TokenKind::Lte, start);
                }
                '<' => self.emit(TokenKind::Lt, start),

                '+' => self.emit(TokenKind::Plus, start),
                '-' => self.emit(TokenKind::Minus, start),
                '*' => self.emit(TokenKind::Star, start),
                '/' => self.emit(TokenKind::Slash, start),
                '%' => self.emit(TokenKind::Percent, start),

                // Delimiters
                '(' => self.emit(TokenKind::LParen, start),
                ')' => self.emit(TokenKind::RParen, start),
                '[' => self.emit(TokenKind::LBracket, start),
                ']' => self.emit(TokenKind::RBracket, start),
                ',' => self.emit(TokenKind::Comma, start),
                '.' => self.emit(TokenKind::Dot, start),
                ':' => self.emit(TokenKind::Colon, start),

                '|' if self.peek() == Some('>') => {
                    self.advance();
                    self.emit(TokenKind::PipeArrow, start);
                }
                '|' => self.emit(TokenKind::Pipe, start),

                // Identifiers & Keywords
                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.read_ident(c);
                    let kind = match ident.to_uppercase().as_str() {
                        "FROM" => TokenKind::From,
                        "WHERE" => TokenKind::Where,
                        "WITHIN" => TokenKind::Within,
                        "COMPUTE" => TokenKind::Compute,
                        "SHOW" => TokenKind::Show,
                        "CORRELATE" => TokenKind::Correlate,
                        "ON" => TokenKind::On,
                        "GROUP" => {
                            // Check for GROUP BY
                            self.skip_whitespace();
                            let saved = self.pos;
                            if self.pos < self.input.len() {
                                if let Some(c) = self.advance() {
                                    let next = self.read_ident(c);
                                    if next.to_uppercase() == "BY" {
                                        TokenKind::GroupBy
                                    } else {
                                        self.pos = saved;
                                        TokenKind::Ident(ident)
                                    }
                                } else {
                                    self.pos = saved;
                                    TokenKind::Ident(ident)
                                }
                            } else {
                                TokenKind::Ident(ident)
                            }
                        }
                        "HAVING" => TokenKind::Having,
                        "AS" => TokenKind::As,
                        "AND" => TokenKind::And,
                        "OR" => TokenKind::Or,
                        "NOT" => TokenKind::Not,
                        "IN" => TokenKind::In,
                        "BY" => TokenKind::By,
                        "LAST" => TokenKind::Last,
                        "TO" => TokenKind::To,
                        "TODAY" => TokenKind::Today,
                        "THIS_WEEK" => TokenKind::ThisWeek,
                        "PARSE" => TokenKind::Parse,
                        "FILTER" => TokenKind::Filter,
                        "DEFINE" => TokenKind::Define,
                        "USE" => TokenKind::Use,
                        "EXPLAIN" => TokenKind::Explain,
                        "VALIDATE" => TokenKind::Validate,
                        "FILL" => TokenKind::Fill,
                        "NATIVE" => TokenKind::Native,
                        "CONTAINS" => TokenKind::Contains,
                        "STARTS" => {
                            // Check for STARTS WITH
                            self.skip_whitespace();
                            let saved = self.pos;
                            if self.pos < self.input.len() {
                                if let Some(c) = self.advance() {
                                    let next = self.read_ident(c);
                                    if next.to_uppercase() == "WITH" {
                                        TokenKind::StartsWith
                                    } else {
                                        self.pos = saved;
                                        TokenKind::Ident(ident)
                                    }
                                } else {
                                    self.pos = saved;
                                    TokenKind::Ident(ident)
                                }
                            } else {
                                TokenKind::Ident(ident)
                            }
                        }
                        "MATCHES" => TokenKind::Matches,
                        _ => TokenKind::Ident(ident),
                    };
                    self.emit(kind, start);
                }

                _ => {
                    return Err(LexError::UnexpectedChar { ch, pos: start });
                }
            }
        }

        self.emit(TokenKind::Eof, self.pos);
        Ok(())
    }
}

// ─── Public API ────────────────────────────────────────────────────────────────

pub fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(input);
    lexer.tokenize()?;
    Ok(lexer.tokens)
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn token_kinds(input: &str) -> Vec<TokenKind> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| *k != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn test_basic_keywords() {
        let kinds =
            token_kinds("FROM metrics WHERE service = \"nginx\" WITHIN last 5m SHOW timeseries");
        assert_eq!(
            kinds,
            vec![
                TokenKind::From,
                TokenKind::Ident("metrics".into()),
                TokenKind::Where,
                TokenKind::Ident("service".into()),
                TokenKind::Eq,
                TokenKind::StringLit("nginx".into()),
                TokenKind::Within,
                TokenKind::Last,
                TokenKind::DurationLit("5m".into()),
                TokenKind::Show,
                TokenKind::Ident("timeseries".into()),
            ]
        );
    }

    #[test]
    fn test_operators() {
        let kinds = token_kinds("a >= 10 AND b != 20 OR c =~ \"foo.*\"");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("a".into()),
                TokenKind::Gte,
                TokenKind::NumberLit(10.0),
                TokenKind::And,
                TokenKind::Ident("b".into()),
                TokenKind::Neq,
                TokenKind::NumberLit(20.0),
                TokenKind::Or,
                TokenKind::Ident("c".into()),
                TokenKind::RegexMatch,
                TokenKind::StringLit("foo.*".into()),
            ]
        );
    }

    #[test]
    fn test_dotted_identifiers() {
        let kinds = token_kinds("metrics.cpu > 80");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("metrics".into()),
                TokenKind::Dot,
                TokenKind::Ident("cpu".into()),
                TokenKind::Gt,
                TokenKind::NumberLit(80.0),
            ]
        );
    }

    #[test]
    fn test_duration_literals() {
        let kinds = token_kinds("500ms 30s 5m 1h 24h 7d");
        assert_eq!(
            kinds,
            vec![
                TokenKind::DurationLit("500ms".into()),
                TokenKind::DurationLit("30s".into()),
                TokenKind::DurationLit("5m".into()),
                TokenKind::DurationLit("1h".into()),
                TokenKind::DurationLit("24h".into()),
                TokenKind::DurationLit("7d".into()),
            ]
        );
    }

    #[test]
    fn test_group_by() {
        let kinds = token_kinds("COMPUTE rate(value, 1m) GROUP BY service");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Compute,
                TokenKind::Ident("rate".into()),
                TokenKind::LParen,
                TokenKind::Ident("value".into()),
                TokenKind::Comma,
                TokenKind::DurationLit("1m".into()),
                TokenKind::RParen,
                TokenKind::GroupBy,
                TokenKind::Ident("service".into()),
            ]
        );
    }

    #[test]
    fn test_comments() {
        let kinds = token_kinds("FROM metrics -- this is a comment\nWHERE service = \"api\"");
        assert_eq!(
            kinds,
            vec![
                TokenKind::From,
                TokenKind::Ident("metrics".into()),
                TokenKind::Where,
                TokenKind::Ident("service".into()),
                TokenKind::Eq,
                TokenKind::StringLit("api".into()),
            ]
        );
    }

    #[test]
    fn test_pipe_arrow() {
        let kinds = token_kinds("metrics |> FILTER cpu > 80 |> SHOW timeseries");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("metrics".into()),
                TokenKind::PipeArrow,
                TokenKind::Filter,
                TokenKind::Ident("cpu".into()),
                TokenKind::Gt,
                TokenKind::NumberLit(80.0),
                TokenKind::PipeArrow,
                TokenKind::Show,
                TokenKind::Ident("timeseries".into()),
            ]
        );
    }

    #[test]
    fn test_contains_startswith_matches() {
        let kinds = token_kinds("message CONTAINS \"error\" AND host STARTS WITH \"prod-\"");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("message".into()),
                TokenKind::Contains,
                TokenKind::StringLit("error".into()),
                TokenKind::And,
                TokenKind::Ident("host".into()),
                TokenKind::StartsWith,
                TokenKind::StringLit("prod-".into()),
            ]
        );
    }

    #[test]
    fn test_correlate_clause() {
        let kinds = token_kinds("CORRELATE ON service, host WITHIN 30s");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Correlate,
                TokenKind::On,
                TokenKind::Ident("service".into()),
                TokenKind::Comma,
                TokenKind::Ident("host".into()),
                TokenKind::Within,
                TokenKind::DurationLit("30s".into()),
            ]
        );
    }

    #[test]
    fn test_backend_hint() {
        let kinds = token_kinds("FROM metrics:victoria, logs:loki");
        assert_eq!(
            kinds,
            vec![
                TokenKind::From,
                TokenKind::Ident("metrics".into()),
                TokenKind::Colon,
                TokenKind::Ident("victoria".into()),
                TokenKind::Comma,
                TokenKind::Ident("logs".into()),
                TokenKind::Colon,
                TokenKind::Ident("loki".into()),
            ]
        );
    }

    #[test]
    fn test_full_query() {
        // Real AETHERIS use case
        let query = r#"
            FROM metrics:victoria, logs:loki
            WHERE labels.device_type = "router"
              AND metrics.__name__ = "ifInErrors"
              AND logs.message CONTAINS "link down"
            WITHIN last 5m
            CORRELATE ON host WITHIN 60s
            SHOW timeline
        "#;
        let tokens = tokenize(query).unwrap();
        assert!(tokens.len() > 10);
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Eof);
    }

    #[test]
    fn test_error_unterminated_string() {
        let result = tokenize("service = \"unterminated");
        assert!(result.is_err());
        match result.unwrap_err() {
            LexError::UnterminatedString { .. } => {}
            e => panic!("Expected UnterminatedString, got {:?}", e),
        }
    }

    #[test]
    fn test_regex_operators() {
        let kinds = token_kinds("service =~ \"api.*\" AND status !~ \"2..\"");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("service".into()),
                TokenKind::RegexMatch,
                TokenKind::StringLit("api.*".into()),
                TokenKind::And,
                TokenKind::Ident("status".into()),
                TokenKind::RegexNoMatch,
                TokenKind::StringLit("2..".into()),
            ]
        );
    }

    #[test]
    fn test_in_operator_with_list() {
        let kinds = token_kinds("service IN [\"nginx\", \"envoy\"]");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("service".into()),
                TokenKind::In,
                TokenKind::LBracket,
                TokenKind::StringLit("nginx".into()),
                TokenKind::Comma,
                TokenKind::StringLit("envoy".into()),
                TokenKind::RBracket,
            ]
        );
    }
}
