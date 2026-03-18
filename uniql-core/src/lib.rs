pub mod ast;
pub mod bind;
pub mod config;
pub mod expand;
pub mod lexer;
pub mod normalize;
pub mod parser;
pub mod semantic;
pub mod transpiler;

// ─── Unified Error Type ──────────────────────────────────────────────────────

/// Typed error enum for the public API.
/// Callers can distinguish which pipeline stage failed.
#[derive(Debug)]
pub enum UniqlError {
    Lex(lexer::LexError),
    Parse(parser::ParseError),
    Expand(expand::ExpandError),
    Semantic(semantic::SemanticError),
    Bind(String),
    Normalize(String),
    Transpile(transpiler::TranspileError),
}

impl std::fmt::Display for UniqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UniqlError::Lex(e) => write!(f, "Lex error: {}", e),
            UniqlError::Parse(e) => write!(f, "Parse error: {}", e),
            UniqlError::Expand(e) => write!(f, "Expand error: {}", e),
            UniqlError::Semantic(e) => write!(f, "Semantic error: {}", e),
            UniqlError::Bind(e) => write!(f, "Bind error: {}", e),
            UniqlError::Normalize(e) => write!(f, "Normalize error: {}", e),
            UniqlError::Transpile(e) => write!(f, "Transpile error: {}", e),
        }
    }
}

impl std::error::Error for UniqlError {}

impl From<lexer::LexError> for UniqlError {
    fn from(e: lexer::LexError) -> Self { UniqlError::Lex(e) }
}
impl From<parser::ParseError> for UniqlError {
    fn from(e: parser::ParseError) -> Self { UniqlError::Parse(e) }
}
impl From<expand::ExpandError> for UniqlError {
    fn from(e: expand::ExpandError) -> Self { UniqlError::Expand(e) }
}
impl From<semantic::SemanticError> for UniqlError {
    fn from(e: semantic::SemanticError) -> Self { UniqlError::Semantic(e) }
}
impl From<transpiler::TranspileError> for UniqlError {
    fn from(e: transpiler::TranspileError) -> Self { UniqlError::Transpile(e) }
}

// ─── Public API (typed errors) ───────────────────────────────────────────────

/// Parse a UNIQL query string and return the AST.
pub fn parse(input: &str) -> Result<ast::Query, UniqlError> {
    check_query_size(input)?;
    let tokens = lexer::tokenize(input)?;
    let ast = parser::parse(tokens)?;
    Ok(ast)
}

/// Full pipeline: parse → expand macros → validate → return clean AST.
pub fn prepare(input: &str) -> Result<ast::Query, UniqlError> {
    let ast = parse(input)?;
    let expanded = expand::expand(&ast)?;
    let _warnings = semantic::validate(&expanded)?;
    Ok(expanded)
}

/// Extended pipeline: parse → expand → validate → bind → return BoundQuery.
pub fn prepare_bound(input: &str) -> Result<bind::BoundQuery, UniqlError> {
    let ast = prepare(input)?;
    bind::bind(&ast).map_err(UniqlError::Bind)
}

/// Full normalized pipeline: parse → expand → validate → bind → normalize.
pub fn prepare_normalized(input: &str) -> Result<normalize::NormalizedQuery, UniqlError> {
    let bound = prepare_bound(input)?;
    normalize::normalize(bound).map_err(UniqlError::Normalize)
}

/// Parse and transpile a UNIQL query to PromQL.
pub fn to_promql(input: &str) -> Result<String, UniqlError> {
    let ast = prepare(input)?;
    let output = transpiler::promql::transpile(&ast)?;
    Ok(output)
}

/// Parse and transpile a UNIQL query to LogsQL (VictoriaLogs).
pub fn to_logsql(input: &str) -> Result<String, UniqlError> {
    let ast = prepare(input)?;
    let output = transpiler::logsql::transpile(&ast)?;
    Ok(output)
}

/// Parse and transpile a UNIQL query to LogQL (Loki).
pub fn to_logql(input: &str) -> Result<String, UniqlError> {
    let ast = prepare(input)?;
    let output = transpiler::logql::transpile(&ast)?;
    Ok(output)
}

// ─── Legacy API (String errors, backward compatible) ─────────────────────────

/// Parse (legacy, returns String error for backward compatibility).
pub fn parse_str(input: &str) -> Result<ast::Query, String> {
    parse(input).map_err(|e| e.to_string())
}

/// Prepare (legacy, returns String error for backward compatibility).
pub fn prepare_str(input: &str) -> Result<ast::Query, String> {
    prepare(input).map_err(|e| e.to_string())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn check_query_size(input: &str) -> Result<(), UniqlError> {
    if input.len() > config::MAX_QUERY_SIZE {
        Err(UniqlError::Parse(parser::ParseError::QueryTooLarge {
            len: input.len(),
            max: config::MAX_QUERY_SIZE,
        }))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typed_error_lex() {
        let result = parse("FROM metrics WHERE @@@");
        assert!(matches!(result, Err(UniqlError::Lex(_))));
    }

    #[test]
    fn test_typed_error_parse() {
        let result = parse("FROM FROM FROM");
        assert!(matches!(result, Err(UniqlError::Parse(_))));
    }

    #[test]
    fn test_typed_error_semantic() {
        // Multi-signal without CORRELATE
        let result = prepare("FROM metrics, logs");
        assert!(matches!(result, Err(UniqlError::Semantic(_))));
    }

    #[test]
    fn test_query_too_large() {
        let huge = "a".repeat(config::MAX_QUERY_SIZE + 1);
        let result = parse(&huge);
        assert!(matches!(result, Err(UniqlError::Parse(parser::ParseError::QueryTooLarge { .. }))));
    }

    #[test]
    fn test_normal_query_ok() {
        let result = parse("FROM metrics WHERE service = \"api\"");
        assert!(result.is_ok());
    }

    #[test]
    fn test_error_display() {
        let err = UniqlError::Bind("test error".to_string());
        assert_eq!(err.to_string(), "Bind error: test error");
    }

    #[test]
    fn test_legacy_api_compat() {
        let result = parse_str("FROM metrics WHERE service = \"api\"");
        assert!(result.is_ok());

        let result = parse_str("FROM FROM FROM");
        assert!(result.is_err());
        // Legacy API returns String error
        assert!(result.unwrap_err().contains("Parse error"));
    }
}
