//! UNIQL Parser — Recursive Descent + Pratt Parsing
//!
//! Converts a token stream into a typed AST.
//! Uses Pratt parsing for expressions (operator precedence).

use crate::ast::*;
use crate::lexer::{Token, TokenKind};
use thiserror::Error;

// ─── Errors ────────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Expected {expected}, found {found:?}")]
    Expected { expected: String, found: TokenKind },

    #[error("Unexpected token {token:?}")]
    Unexpected { token: TokenKind },

    #[error("Unknown SHOW format '{format}'. Expected: timeseries, table, timeline, heatmap, flamegraph, count, alert, topology")]
    UnknownShowFormat { format: String },

    #[error("Expression nesting depth exceeds maximum ({max}). Simplify your query.")]
    MaxDepthExceeded { max: usize },

    #[error("Query too large ({len} bytes, max {max} bytes)")]
    QueryTooLarge { len: usize, max: usize },
}

// ─── Parser ────────────────────────────────────────────────────────────────────

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos];
        self.pos += 1;
        token
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<&Token, ParseError> {
        if self.peek() == expected {
            Ok(self.advance())
        } else {
            Err(ParseError::Expected {
                expected: format!("{:?}", expected),
                found: self.peek().clone(),
            })
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    // ─── Top-Level Query ───────────────────────────────────────────────────

    fn parse_query(&mut self) -> Result<Query, ParseError> {
        let mut query = Query::new();

        while !self.is_at_end() {
            match self.peek().clone() {
                TokenKind::Define => {
                    query.defines.push(self.parse_define()?);
                }
                TokenKind::From => {
                    query.from = Some(self.parse_from()?);
                }
                TokenKind::Where | TokenKind::Filter => {
                    query.where_clause = Some(self.parse_where()?);
                }
                TokenKind::Within => {
                    query.within = Some(self.parse_within()?);
                }
                TokenKind::Parse => {
                    query.parse = Some(self.parse_parse()?);
                }
                TokenKind::Compute => {
                    query.compute = Some(self.parse_compute()?);
                }
                TokenKind::GroupBy => {
                    query.group_by = Some(self.parse_group_by()?);
                }
                TokenKind::Having => {
                    query.having = Some(self.parse_having()?);
                }
                TokenKind::Correlate => {
                    query.correlate = Some(self.parse_correlate()?);
                }
                TokenKind::Show => {
                    query.show = Some(self.parse_show()?);
                }
                // |> pipe syntax: consume pipe arrow, next token must be a clause keyword
                TokenKind::PipeArrow => {
                    self.advance(); // consume |>
                                    // Next iteration will parse the clause keyword
                }
                _ => {
                    return Err(ParseError::Unexpected {
                        token: self.peek().clone(),
                    });
                }
            }
        }

        Ok(query)
    }

    // ─── FROM Clause ───────────────────────────────────────────────────────

    fn parse_from(&mut self) -> Result<FromClause, ParseError> {
        self.advance(); // consume FROM
        let mut sources = Vec::new();

        loop {
            let source = self.parse_data_source()?;
            sources.push(source);

            if self.peek() == &TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(FromClause { sources })
    }

    fn parse_data_source(&mut self) -> Result<DataSource, ParseError> {
        let name = self.expect_ident()?;
        let signal_type = SignalType::parse_signal(&name);

        // Check for backend hint: metrics:victoria
        let backend_hint = if self.peek() == &TokenKind::Colon {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        // Check for alias: AS m
        let alias = if self.peek() == &TokenKind::As {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        Ok(DataSource {
            signal_type,
            backend_hint,
            alias,
        })
    }

    // ─── WHERE Clause ──────────────────────────────────────────────────────

    fn parse_where(&mut self) -> Result<WhereClause, ParseError> {
        self.advance(); // consume WHERE
        let condition = self.parse_expr(0)?;
        Ok(WhereClause { condition })
    }

    // ─── PARSE Clause ──────────────────────────────────────────────────────

    fn parse_parse(&mut self) -> Result<ParseClause, ParseError> {
        self.advance(); // consume PARSE

        let mode_name = self.expect_ident()?;
        let (mode, pattern) = match mode_name.to_lowercase().as_str() {
            "json" => (ParseMode::Json, None),
            "logfmt" => (ParseMode::Logfmt, None),
            "pattern" => {
                let pat = self.expect_string()?;
                (ParseMode::Pattern(pat.clone()), Some(pat))
            }
            "regexp" | "regex" => {
                let pat = self.expect_string()?;
                (ParseMode::Regexp(pat.clone()), Some(pat))
            }
            _ => {
                return Err(ParseError::Expected {
                    expected: "json, logfmt, pattern, or regexp".into(),
                    found: TokenKind::Ident(mode_name),
                });
            }
        };

        Ok(ParseClause { mode, pattern })
    }

    // ─── WITHIN Clause ─────────────────────────────────────────────────────

    fn parse_within(&mut self) -> Result<WithinClause, ParseError> {
        self.advance(); // consume WITHIN

        match self.peek().clone() {
            TokenKind::Last => {
                self.advance();
                let duration = self.expect_duration()?;
                Ok(WithinClause::Last(duration))
            }
            TokenKind::Today => {
                self.advance();
                Ok(WithinClause::Today)
            }
            TokenKind::ThisWeek => {
                self.advance();
                Ok(WithinClause::ThisWeek)
            }
            TokenKind::StringLit(from) => {
                self.advance();
                self.expect(&TokenKind::To)?;
                let to = match self.peek().clone() {
                    TokenKind::StringLit(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        return Err(ParseError::Expected {
                            expected: "end date string".into(),
                            found: self.peek().clone(),
                        })
                    }
                };
                Ok(WithinClause::Range { from, to })
            }
            _ => Err(ParseError::Expected {
                expected: "last, today, this_week, or date range".into(),
                found: self.peek().clone(),
            }),
        }
    }

    // ─── COMPUTE Clause ────────────────────────────────────────────────────

    fn parse_compute(&mut self) -> Result<ComputeClause, ParseError> {
        self.advance(); // consume COMPUTE
        let mut functions = Vec::new();

        loop {
            let func = self.parse_compute_function()?;
            functions.push(func);

            if self.peek() == &TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(ComputeClause { functions })
    }

    fn parse_compute_function(&mut self) -> Result<ComputeFunction, ParseError> {
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;

        let mut args = Vec::new();
        if self.peek() != &TokenKind::RParen {
            loop {
                let arg = self.parse_expr(0)?;
                args.push(arg);
                if self.peek() == &TokenKind::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen)?;

        // Check for inline GROUP BY: COMPUTE rate(value, 1m) BY service
        // This is handled at a higher level, but check for AS alias
        let alias = if self.peek() == &TokenKind::As {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        Ok(ComputeFunction { name, args, alias })
    }

    // ─── GROUP BY Clause ───────────────────────────────────────────────────

    fn parse_group_by(&mut self) -> Result<GroupByClause, ParseError> {
        self.advance(); // consume GROUP BY (already combined in lexer)
        let mut fields = Vec::new();

        loop {
            let field = self.parse_expr(0)?;
            fields.push(field);
            if self.peek() == &TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }

        Ok(GroupByClause { fields })
    }

    // ─── HAVING Clause ─────────────────────────────────────────────────────

    fn parse_having(&mut self) -> Result<HavingClause, ParseError> {
        self.advance(); // consume HAVING
        let condition = self.parse_expr(0)?;
        Ok(HavingClause { condition })
    }

    // ─── CORRELATE Clause ──────────────────────────────────────────────────

    fn parse_correlate(&mut self) -> Result<CorrelateClause, ParseError> {
        self.advance(); // consume CORRELATE
        self.expect(&TokenKind::On)?;

        let mut on_fields = Vec::new();
        loop {
            on_fields.push(self.expect_ident()?);
            if self.peek() == &TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }

        let within = if self.peek() == &TokenKind::Within {
            self.advance();
            Some(self.expect_duration()?)
        } else {
            None
        };

        // Check for SKEW_TOLERANCE
        let skew_tolerance = if let TokenKind::Ident(ref s) = self.peek().clone() {
            if s.to_uppercase() == "SKEW_TOLERANCE" {
                self.advance();
                Some(self.expect_duration()?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(CorrelateClause {
            on_fields,
            within,
            skew_tolerance,
        })
    }

    // ─── SHOW Clause ───────────────────────────────────────────────────────

    // ─── DEFINE Clause ──────────────────────────────────────────────────────

    fn parse_define(&mut self) -> Result<DefineClause, ParseError> {
        self.advance(); // consume DEFINE
        let name = self.expect_ident()?;

        // Optional parameters: DEFINE error_rate(metric, window) = ...
        let mut params = Vec::new();
        if self.peek() == &TokenKind::LParen {
            self.advance(); // consume (
            if self.peek() != &TokenKind::RParen {
                loop {
                    params.push(self.expect_ident()?);
                    if self.peek() == &TokenKind::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen)?;
        }

        // Expect =
        self.expect(&TokenKind::Eq)?;

        // Parse the body expression
        let body = self.parse_expr(0)?;

        Ok(DefineClause { name, params, body })
    }

    // ─── SHOW Clause ───────────────────────────────────────────────────────

    fn parse_show(&mut self) -> Result<ShowClause, ParseError> {
        self.advance(); // consume SHOW
        let format_name = self.expect_ident()?;

        let format =
            ShowFormat::parse_format(&format_name).ok_or(ParseError::UnknownShowFormat {
                format: format_name,
            })?;

        Ok(ShowClause { format })
    }

    // ─── Expression Parser (Pratt Parsing) ─────────────────────────────────

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        self.depth += 1;
        if self.depth > crate::config::MAX_EXPR_DEPTH {
            return Err(ParseError::MaxDepthExceeded {
                max: crate::config::MAX_EXPR_DEPTH,
            });
        }
        let result = self.parse_expr_inner(min_bp);
        self.depth -= 1;
        result
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix()?;

        loop {
            let op = match self.peek() {
                TokenKind::And => BinaryOp::And,
                TokenKind::Or => BinaryOp::Or,
                TokenKind::Eq => BinaryOp::Eq,
                TokenKind::Neq => BinaryOp::Neq,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gte => BinaryOp::Gte,
                TokenKind::Lte => BinaryOp::Lte,
                TokenKind::RegexMatch => BinaryOp::RegexMatch,
                TokenKind::RegexNoMatch => BinaryOp::RegexNoMatch,
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                // String match operators
                TokenKind::Contains => {
                    self.advance();
                    let pattern = self.expect_string()?;
                    lhs = Expr::StringMatch {
                        expr: Box::new(lhs),
                        op: StringMatchOp::Contains,
                        pattern,
                    };
                    continue;
                }
                TokenKind::StartsWith => {
                    self.advance();
                    let pattern = self.expect_string()?;
                    lhs = Expr::StringMatch {
                        expr: Box::new(lhs),
                        op: StringMatchOp::StartsWith,
                        pattern,
                    };
                    continue;
                }
                TokenKind::Matches => {
                    self.advance();
                    let pattern = self.expect_string()?;
                    lhs = Expr::StringMatch {
                        expr: Box::new(lhs),
                        op: StringMatchOp::Matches,
                        pattern,
                    };
                    continue;
                }
                // IN operator
                TokenKind::In => {
                    self.advance();
                    self.expect(&TokenKind::LBracket)?;
                    let mut list = Vec::new();
                    if self.peek() != &TokenKind::RBracket {
                        loop {
                            list.push(self.parse_expr(0)?);
                            if self.peek() == &TokenKind::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RBracket)?;
                    lhs = Expr::InList {
                        expr: Box::new(lhs),
                        list,
                        negated: false,
                    };
                    continue;
                }
                _ => break,
            };

            let (l_bp, r_bp) = infix_binding_power(&op);
            if l_bp < min_bp {
                break;
            }

            self.advance(); // consume operator
            let rhs = self.parse_expr(r_bp)?;
            lhs = Expr::BinaryOp {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            TokenKind::Not => {
                self.advance();
                let expr = self.parse_expr(prefix_binding_power())?;
                Ok(Expr::Not(Box::new(expr)))
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr(0)?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.advance();

                // Check for function call: name(...)
                if self.peek() == &TokenKind::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &TokenKind::RParen {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if self.peek() == &TokenKind::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Expr::FunctionCall { name, args });
                }

                // Check for qualified identifier: metrics.cpu.usage
                if self.peek() == &TokenKind::Dot {
                    let mut parts = vec![name];
                    while self.peek() == &TokenKind::Dot {
                        self.advance();
                        parts.push(self.expect_ident()?);
                    }
                    return Ok(Expr::QualifiedIdent(parts));
                }

                Ok(Expr::Ident(name))
            }
            TokenKind::StringLit(ref s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::StringLit(s))
            }
            TokenKind::NumberLit(n) => {
                self.advance();
                Ok(Expr::NumberLit(n))
            }
            TokenKind::DurationLit(ref d) => {
                let d = d.clone();
                self.advance();
                Ok(Expr::DurationLit(d))
            }
            TokenKind::Star => {
                self.advance();
                Ok(Expr::Star)
            }
            // NATIVE("backend", "query") or NATIVE("query")
            TokenKind::Native => {
                self.advance();
                self.expect(&TokenKind::LParen)?;
                let first = self.expect_string()?;
                let (backend, query) = if self.peek() == &TokenKind::Comma {
                    self.advance();
                    let query = self.expect_string()?;
                    (Some(first), query)
                } else {
                    (None, first)
                };
                self.expect(&TokenKind::RParen)?;
                Ok(Expr::Native { backend, query })
            }
            _ => Err(ParseError::Unexpected {
                token: self.peek().clone(),
            }),
        }
    }

    // ─── Helpers ───────────────────────────────────────────────────────────

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(ParseError::Expected {
                expected: "identifier".into(),
                found: other,
            }),
        }
    }

    fn expect_string(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(ParseError::Expected {
                expected: "string literal".into(),
                found: other,
            }),
        }
    }

    fn expect_duration(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            TokenKind::DurationLit(d) => {
                self.advance();
                Ok(d)
            }
            other => Err(ParseError::Expected {
                expected: "duration (e.g., 5m, 1h, 30s)".into(),
                found: other,
            }),
        }
    }
}

// ─── Pratt Parsing: Binding Powers ─────────────────────────────────────────────

fn infix_binding_power(op: &BinaryOp) -> (u8, u8) {
    match op {
        BinaryOp::Or => (1, 2),
        BinaryOp::And => (3, 4),
        BinaryOp::Eq
        | BinaryOp::Neq
        | BinaryOp::Gt
        | BinaryOp::Lt
        | BinaryOp::Gte
        | BinaryOp::Lte
        | BinaryOp::RegexMatch
        | BinaryOp::RegexNoMatch => (5, 6),
        BinaryOp::Add | BinaryOp::Sub => (7, 8),
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => (9, 10),
    }
}

fn prefix_binding_power() -> u8 {
    11
}

// ─── Public API ────────────────────────────────────────────────────────────────

pub fn parse(tokens: Vec<Token>) -> Result<Query, ParseError> {
    let mut parser = Parser::new(tokens);
    parser.parse_query()
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    fn parse_query(input: &str) -> Query {
        let tokens = lexer::tokenize(input).unwrap();
        parse(tokens).unwrap()
    }

    #[test]
    fn test_simple_from() {
        let q = parse_query("FROM metrics");
        let from = q.from.unwrap();
        assert_eq!(from.sources.len(), 1);
        assert_eq!(from.sources[0].signal_type, SignalType::Metrics);
        assert_eq!(from.sources[0].backend_hint, None);
    }

    #[test]
    fn test_from_with_backend_hint() {
        let q = parse_query("FROM metrics:victoria, logs:loki");
        let from = q.from.unwrap();
        assert_eq!(from.sources.len(), 2);
        assert_eq!(from.sources[0].backend_hint, Some("victoria".into()));
        assert_eq!(from.sources[1].signal_type, SignalType::Logs);
        assert_eq!(from.sources[1].backend_hint, Some("loki".into()));
    }

    #[test]
    fn test_from_with_alias() {
        let q = parse_query("FROM metrics AS m, logs AS l");
        let from = q.from.unwrap();
        assert_eq!(from.sources[0].alias, Some("m".into()));
        assert_eq!(from.sources[1].alias, Some("l".into()));
    }

    #[test]
    fn test_where_simple() {
        let q = parse_query("FROM metrics WHERE service = \"nginx\"");
        let wc = q.where_clause.unwrap();
        match wc.condition {
            Expr::BinaryOp {
                left, op, right, ..
            } => {
                assert_eq!(op, BinaryOp::Eq);
                match *left {
                    Expr::Ident(ref name) => assert_eq!(name, "service"),
                    _ => panic!("Expected Ident"),
                }
                match *right {
                    Expr::StringLit(ref s) => assert_eq!(s, "nginx"),
                    _ => panic!("Expected StringLit"),
                }
            }
            _ => panic!("Expected BinaryOp"),
        }
    }

    #[test]
    fn test_where_compound() {
        let q = parse_query(
            "FROM metrics WHERE service = \"nginx\" AND env = \"prod\" OR status > 400",
        );
        let wc = q.where_clause.unwrap();
        assert_eq!(wc.condition_count(), 3);
    }

    #[test]
    fn test_where_qualified_ident() {
        let q = parse_query("FROM metrics WHERE metrics.cpu > 80");
        let wc = q.where_clause.unwrap();
        match wc.condition {
            Expr::BinaryOp { left, .. } => match *left {
                Expr::QualifiedIdent(ref parts) => {
                    assert_eq!(parts, &vec!["metrics".to_string(), "cpu".to_string()]);
                }
                _ => panic!("Expected QualifiedIdent"),
            },
            _ => panic!("Expected BinaryOp"),
        }
    }

    #[test]
    fn test_where_contains() {
        let q = parse_query("FROM logs WHERE message CONTAINS \"error\"");
        let wc = q.where_clause.unwrap();
        match wc.condition {
            Expr::StringMatch { op, pattern, .. } => {
                assert_eq!(op, StringMatchOp::Contains);
                assert_eq!(pattern, "error");
            }
            _ => panic!("Expected StringMatch"),
        }
    }

    #[test]
    fn test_where_in_list() {
        let q = parse_query("FROM metrics WHERE service IN [\"nginx\", \"envoy\", \"haproxy\"]");
        let wc = q.where_clause.unwrap();
        match wc.condition {
            Expr::InList { list, negated, .. } => {
                assert_eq!(list.len(), 3);
                assert!(!negated);
            }
            _ => panic!("Expected InList"),
        }
    }

    #[test]
    fn test_within_last() {
        let q = parse_query("FROM metrics WITHIN last 5m");
        match q.within.unwrap() {
            WithinClause::Last(d) => assert_eq!(d, "5m"),
            _ => panic!("Expected Last"),
        }
    }

    #[test]
    fn test_within_range() {
        let q = parse_query("FROM metrics WITHIN \"2025-03-01\" TO \"2025-03-10\"");
        match q.within.unwrap() {
            WithinClause::Range { from, to } => {
                assert_eq!(from, "2025-03-01");
                assert_eq!(to, "2025-03-10");
            }
            _ => panic!("Expected Range"),
        }
    }

    #[test]
    fn test_compute() {
        let q = parse_query("FROM metrics COMPUTE rate(value, 1m), avg(cpu)");
        let compute = q.compute.unwrap();
        assert_eq!(compute.functions.len(), 2);
        assert_eq!(compute.functions[0].name, "rate");
        assert_eq!(compute.functions[0].args.len(), 2);
        assert_eq!(compute.functions[1].name, "avg");
    }

    #[test]
    fn test_group_by() {
        let q = parse_query("FROM metrics COMPUTE count() GROUP BY service, host");
        let gb = q.group_by.unwrap();
        assert_eq!(gb.fields.len(), 2);
    }

    #[test]
    fn test_having() {
        let q =
            parse_query("FROM metrics COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01");
        let having = q.having.unwrap();
        match having.condition {
            Expr::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Gt),
            _ => panic!("Expected BinaryOp"),
        }
    }

    #[test]
    fn test_correlate() {
        let q = parse_query("FROM metrics, logs CORRELATE ON service, host WITHIN 30s");
        let corr = q.correlate.unwrap();
        assert_eq!(corr.on_fields, vec!["service", "host"]);
        assert_eq!(corr.within, Some("30s".into()));
    }

    #[test]
    fn test_show_formats() {
        for (input, expected) in [
            ("SHOW timeseries", ShowFormat::Timeseries),
            ("SHOW table", ShowFormat::Table),
            ("SHOW timeline", ShowFormat::Timeline),
            ("SHOW heatmap", ShowFormat::Heatmap),
            ("SHOW flamegraph", ShowFormat::Flamegraph),
            ("SHOW count", ShowFormat::Count),
            ("SHOW alert", ShowFormat::Alert),
            ("SHOW topology", ShowFormat::Topology),
        ] {
            let q = parse_query(&format!("FROM metrics {}", input));
            assert_eq!(q.show.unwrap().format, expected);
        }
    }

    #[test]
    fn test_full_query() {
        let q = parse_query(
            r#"
            FROM metrics:victoria, logs:loki
            WHERE metrics.__name__ = "ifInErrors"
              AND logs.message CONTAINS "link down"
            WITHIN last 5m
            COMPUTE rate(value, 1m) GROUP BY host
            HAVING rate > 0.01
            CORRELATE ON host WITHIN 60s
            SHOW timeline
            "#,
        );

        assert!(q.from.is_some());
        assert!(q.where_clause.is_some());
        assert!(q.within.is_some());
        assert!(q.compute.is_some());
        assert!(q.group_by.is_some());
        assert!(q.having.is_some());
        assert!(q.correlate.is_some());
        assert!(q.show.is_some());

        assert_eq!(q.inferred_signal_types().len(), 2);
        assert_eq!(
            q.clause_summary(),
            "FROM → WHERE → WITHIN → COMPUTE → GROUP BY → HAVING → CORRELATE → SHOW"
        );
    }

    #[test]
    fn test_operator_precedence() {
        // a = 1 AND b = 2 OR c = 3 should parse as (a=1 AND b=2) OR (c=3)
        let q = parse_query("FROM metrics WHERE a = 1 AND b = 2 OR c = 3");
        let wc = q.where_clause.unwrap();
        match wc.condition {
            Expr::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Or),
            _ => panic!("Top-level should be OR"),
        }
    }

    #[test]
    fn test_regex_operators() {
        let q = parse_query("FROM metrics WHERE service =~ \"api.*\" AND path !~ \"/health.*\"");
        let wc = q.where_clause.unwrap();
        assert_eq!(wc.condition_count(), 2);
    }

    #[test]
    fn test_arithmetic_in_having() {
        let q = parse_query("FROM metrics COMPUTE count() HAVING count * 100 / total > 5");
        assert!(q.having.is_some());
    }

    #[test]
    fn test_comments_ignored() {
        let q = parse_query(
            r#"
            -- This is a comment
            FROM metrics
            -- Another comment
            WHERE service = "api"
            SHOW timeseries
            "#,
        );
        assert!(q.from.is_some());
        assert!(q.where_clause.is_some());
        assert!(q.show.is_some());
    }

    #[test]
    fn test_pipe_syntax_basic() {
        let q = parse_query("FROM metrics |> WHERE service = \"nginx\" |> SHOW timeseries");
        assert!(q.from.is_some());
        assert!(q.where_clause.is_some());
        assert!(q.show.is_some());
    }

    #[test]
    fn test_pipe_syntax_full() {
        let q = parse_query(
            r#"
            FROM metrics:victoria
            |> WHERE __name__ = "http_requests_total" AND env = "prod"
            |> WITHIN last 5m
            |> COMPUTE rate(value, 1m) GROUP BY service
            |> HAVING rate > 0.01
            |> SHOW timeseries
            "#,
        );
        assert!(q.from.is_some());
        assert!(q.where_clause.is_some());
        assert!(q.within.is_some());
        assert!(q.compute.is_some());
        assert!(q.group_by.is_some());
        assert!(q.having.is_some());
        assert!(q.show.is_some());
        assert_eq!(
            q.clause_summary(),
            "FROM → WHERE → WITHIN → COMPUTE → GROUP BY → HAVING → SHOW"
        );
    }

    #[test]
    fn test_pipe_and_sql_produce_same_ast() {
        let sql_q =
            parse_query("FROM metrics WHERE service = \"nginx\" WITHIN last 5m SHOW timeseries");
        let pipe_q = parse_query(
            "FROM metrics |> WHERE service = \"nginx\" |> WITHIN last 5m |> SHOW timeseries",
        );
        assert_eq!(sql_q.clause_summary(), pipe_q.clause_summary());
    }

    #[test]
    fn test_max_depth_exceeded() {
        // Build a deeply nested expression: (((((...))))) — 70 levels deep, exceeds MAX_EXPR_DEPTH=64
        let mut query = "FROM metrics WHERE ".to_string();
        for _ in 0..70 {
            query.push('(');
        }
        query.push_str("x = 1");
        for _ in 0..70 {
            query.push(')');
        }
        let tokens = lexer::tokenize(&query).unwrap();
        let result = parse(tokens);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ParseError::MaxDepthExceeded { .. }),
            "Expected MaxDepthExceeded, got: {:?}",
            err
        );
    }

    #[test]
    fn test_normal_depth_ok() {
        // 5 levels deep should be fine
        let query = "FROM metrics WHERE ((((a = 1 AND b = 2))))";
        let tokens = lexer::tokenize(query).unwrap();
        assert!(parse(tokens).is_ok());
    }
}
