//! UNIQL DEFINE/USE Macro Expansion
//!
//! Expands DEFINE definitions in a query AST before transpilation.
//! This is a pure AST transformation — the transpiler layer never sees DEFINE/USE.
//!
//! Design constraints:
//! - Lexical scoping (visible from declaration to end of query)
//! - No recursion (a DEFINE cannot reference itself)
//! - No conditional logic (not Turing complete)
//! - Expand-before-transpile: output is a clean AST

use crate::ast::*;
use crate::config;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ExpandError {
    pub message: String,
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expansion error: {}", self.message)
    }
}

impl std::error::Error for ExpandError {}

/// A stored definition: name, optional params, body expression
struct Definition {
    params: Vec<String>,
    body: Expr,
}

/// Expand all DEFINE/USE references in a query.
/// Returns a new query with defines resolved and the defines vec cleared.
pub fn expand(query: &Query) -> Result<Query, ExpandError> {
    let mut defs: HashMap<String, Definition> = HashMap::new();
    let mut expansion_count = 0u32;

    // Register all definitions
    for define in &query.defines {
        if defs.contains_key(&define.name) {
            return Err(ExpandError {
                message: format!("Duplicate DEFINE: '{}' is already defined", define.name),
            });
        }
        defs.insert(
            define.name.clone(),
            Definition {
                params: define.params.clone(),
                body: define.body.clone(),
            },
        );
    }

    // If no definitions, return query as-is
    if defs.is_empty() {
        return Ok(query.clone());
    }

    // Expand definitions in each clause
    let mut result = query.clone();
    result.defines.clear(); // Definitions consumed

    if let Some(ref wc) = result.where_clause {
        let expanded = expand_expr(&wc.condition, &defs, &mut expansion_count)?;
        result.where_clause = Some(WhereClause {
            condition: expanded,
        });
    }

    if let Some(ref having) = result.having {
        let expanded = expand_expr(&having.condition, &defs, &mut expansion_count)?;
        result.having = Some(HavingClause {
            condition: expanded,
        });
    }

    Ok(result)
}

/// Recursively expand DEFINE references in an expression.
fn expand_expr(
    expr: &Expr,
    defs: &HashMap<String, Definition>,
    count: &mut u32,
) -> Result<Expr, ExpandError> {
    *count += 1;
    if *count > config::MAX_DEFINE_EXPANSIONS as u32 {
        return Err(ExpandError {
            message: "Maximum expansion depth exceeded. Check for circular DEFINE references."
                .to_string(),
        });
    }

    match expr {
        // Check if this identifier is a DEFINE reference
        Expr::Ident(name) => {
            if let Some(def) = defs.get(name) {
                if !def.params.is_empty() {
                    return Err(ExpandError {
                        message: format!(
                            "'{}' expects {} arguments but was used without parentheses",
                            name,
                            def.params.len()
                        ),
                    });
                }
                // Expand the body (and recursively expand any nested references)
                expand_expr(&def.body, defs, count)
            } else {
                Ok(expr.clone())
            }
        }

        // Function call might be a parameterized DEFINE
        Expr::FunctionCall { name, args } => {
            if let Some(def) = defs.get(name) {
                if def.params.len() != args.len() {
                    return Err(ExpandError {
                        message: format!(
                            "'{}' expects {} arguments, got {}",
                            name,
                            def.params.len(),
                            args.len()
                        ),
                    });
                }
                // Substitute params in body
                let mut substituted = def.body.clone();
                for (param, arg) in def.params.iter().zip(args.iter()) {
                    substituted = substitute_param(&substituted, param, arg);
                }
                expand_expr(&substituted, defs, count)
            } else {
                // Regular function call — expand args
                let expanded_args: Result<Vec<Expr>, ExpandError> =
                    args.iter().map(|a| expand_expr(a, defs, count)).collect();
                Ok(Expr::FunctionCall {
                    name: name.clone(),
                    args: expanded_args?,
                })
            }
        }

        Expr::BinaryOp { left, op, right } => {
            let l = expand_expr(left, defs, count)?;
            let r = expand_expr(right, defs, count)?;
            Ok(Expr::BinaryOp {
                left: Box::new(l),
                op: op.clone(),
                right: Box::new(r),
            })
        }

        Expr::Not(inner) => {
            let expanded = expand_expr(inner, defs, count)?;
            Ok(Expr::Not(Box::new(expanded)))
        }

        Expr::StringMatch {
            expr: inner,
            op,
            pattern,
        } => {
            let expanded = expand_expr(inner, defs, count)?;
            Ok(Expr::StringMatch {
                expr: Box::new(expanded),
                op: op.clone(),
                pattern: pattern.clone(),
            })
        }

        Expr::InList {
            expr: inner,
            list,
            negated,
        } => {
            let expanded_inner = expand_expr(inner, defs, count)?;
            let expanded_list: Result<Vec<Expr>, ExpandError> =
                list.iter().map(|e| expand_expr(e, defs, count)).collect();
            Ok(Expr::InList {
                expr: Box::new(expanded_inner),
                list: expanded_list?,
                negated: *negated,
            })
        }

        // Leaf nodes — no expansion needed
        _ => Ok(expr.clone()),
    }
}

/// Substitute a parameter name with an argument expression throughout a body.
fn substitute_param(expr: &Expr, param: &str, arg: &Expr) -> Expr {
    match expr {
        Expr::Ident(name) if name == param => arg.clone(),
        Expr::BinaryOp { left, op, right } => Expr::BinaryOp {
            left: Box::new(substitute_param(left, param, arg)),
            op: op.clone(),
            right: Box::new(substitute_param(right, param, arg)),
        },
        Expr::Not(inner) => Expr::Not(Box::new(substitute_param(inner, param, arg))),
        Expr::FunctionCall { name, args } => Expr::FunctionCall {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| substitute_param(a, param, arg))
                .collect(),
        },
        Expr::StringMatch {
            expr: inner,
            op,
            pattern,
        } => Expr::StringMatch {
            expr: Box::new(substitute_param(inner, param, arg)),
            op: op.clone(),
            pattern: pattern.clone(),
        },
        Expr::InList {
            expr: inner,
            list,
            negated,
        } => Expr::InList {
            expr: Box::new(substitute_param(inner, param, arg)),
            list: list
                .iter()
                .map(|e| substitute_param(e, param, arg))
                .collect(),
            negated: *negated,
        },
        _ => expr.clone(),
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    fn parse_and_expand(input: &str) -> Result<Query, String> {
        let tokens = lexer::tokenize(input).map_err(|e| e.to_string())?;
        let ast = parser::parse(tokens).map_err(|e| e.to_string())?;
        expand(&ast).map_err(|e| e.to_string())
    }

    #[test]
    fn test_no_defines_passthrough() {
        let q = parse_and_expand("FROM metrics WHERE service = \"nginx\"").unwrap();
        assert!(q.from.is_some());
        assert!(q.defines.is_empty());
    }

    #[test]
    fn test_simple_define_expansion() {
        let q = parse_and_expand(
            "DEFINE prod_svc = service = \"api\" AND env = \"production\" FROM metrics WHERE prod_svc"
        ).unwrap();
        assert!(q.defines.is_empty());
        let wc = q.where_clause.unwrap();
        // Should expand to BinaryOp(service = "api" AND env = "production")
        assert_eq!(wc.condition_count(), 2);
    }

    #[test]
    fn test_duplicate_define_error() {
        let result = parse_and_expand(
            "DEFINE x = service = \"a\" DEFINE x = service = \"b\" FROM metrics WHERE x",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate DEFINE"));
    }

    #[test]
    fn test_undefined_ref_passthrough() {
        // Unknown identifier should pass through (not an error)
        let q = parse_and_expand("FROM metrics WHERE unknown_var = \"test\"").unwrap();
        assert!(q.where_clause.is_some());
    }
}
