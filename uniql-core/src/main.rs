use uniql_core::{lexer, parser, transpiler};

use clap::{Parser as ClapParser, Subcommand};
use colored::*;
use std::process;

#[derive(ClapParser)]
#[command(
    name = "uniql",
    version = "0.2.0",
    about = "UNIQL — Unified Observability Query Language",
    long_about = "Write once, query everything.\nA unified query language for metrics, logs, traces, and events."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a UNIQL query and transpile to backend-native format
    Query {
        /// The UNIQL query string
        query: String,

        /// Target backend: promql, metricsql, logql
        #[arg(short, long, default_value = "promql")]
        backend: String,

        /// Print the AST instead of transpiled query
        #[arg(long)]
        ast: bool,

        /// Print tokens from lexer
        #[arg(long)]
        tokens: bool,
    },

    /// Validate a UNIQL query without executing
    Validate {
        /// The UNIQL query string
        query: String,
    },

    /// Explain the execution plan for a UNIQL query
    Explain {
        /// The UNIQL query string
        query: String,

        /// Target backend
        #[arg(short, long, default_value = "promql")]
        backend: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Query {
            query,
            backend,
            ast: show_ast,
            tokens: show_tokens,
        } => {
            // Tokenize
            let tokens = match lexer::tokenize(&query) {
                Ok(tokens) => tokens,
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            };

            if show_tokens {
                println!("{}", "=== Tokens ===".cyan().bold());
                for token in &tokens {
                    println!("  {:?}", token);
                }
                println!();
            }

            // Parse
            let ast = match parser::parse(tokens) {
                Ok(ast) => ast,
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            };

            if show_ast {
                println!("{}", "=== AST ===".cyan().bold());
                println!("{}", serde_json::to_string_pretty(&ast).unwrap());
                println!();
            }

            // Transpile using trait-based registry
            let transpiler_impl = match transpiler::get_transpiler(&backend) {
                Some(t) => t,
                None => {
                    eprintln!(
                        "{} Unknown backend '{}'. Supported: promql, metricsql, logql, logsql, vlogs",
                        "Error:".red().bold(),
                        backend
                    );
                    process::exit(1);
                }
            };

            let result = transpiler_impl.transpile(&ast).map(|o| o.native_query);

            match result {
                Ok(output) => {
                    println!("{}", "=== Transpiled Query ===".green().bold());
                    println!("{}", output);
                }
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            }
        }

        Commands::Validate { query } => {
            let tokens = match lexer::tokenize(&query) {
                Ok(t) => t,
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            };

            match parser::parse(tokens) {
                Ok(ast) => {
                    println!("{} Query is valid.", "✓".green().bold());
                    println!("  Signal types: {:?}", ast.inferred_signal_types());
                    println!("  Clauses: {}", ast.clause_summary());
                }
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            }
        }

        Commands::Explain { query, backend } => {
            let tokens = match lexer::tokenize(&query) {
                Ok(t) => t,
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            };

            let ast = match parser::parse(tokens) {
                Ok(ast) => ast,
                Err(e) => {
                    print_error(&query, &e);
                    process::exit(1);
                }
            };

            println!("{}", "=== Execution Plan ===".cyan().bold());
            println!("Target backend: {}", backend.yellow());
            println!("Signal types:   {:?}", ast.inferred_signal_types());
            println!();

            if let Some(ref from) = ast.from {
                println!("1. {} → Scan {:?}", "FROM".bold(), from.sources);
            }
            if let Some(ref where_clause) = ast.where_clause {
                println!(
                    "2. {} → Filter ({} conditions)",
                    "WHERE".bold(),
                    where_clause.condition_count()
                );
            }
            if let Some(ref within) = ast.within {
                println!("3. {} → Time range {:?}", "WITHIN".bold(), within);
            }
            if let Some(ref compute) = ast.compute {
                println!(
                    "4. {} → Aggregate ({} functions)",
                    "COMPUTE".bold(),
                    compute.functions.len()
                );
            }
            if let Some(ref show) = ast.show {
                println!("5. {} → Output format {:?}", "SHOW".bold(), show.format);
            }

            println!();
            if let Some(t) = transpiler::get_transpiler(&backend) {
                if let Ok(output) = t.transpile(&ast) {
                    println!("{}", "Native query:".green().bold());
                    println!("  {}", output.native_query);
                }
            }
        }
    }
}

fn print_error(query: &str, error: &dyn std::fmt::Display) {
    eprintln!("{} {}", "Error:".red().bold(), error);
    eprintln!();
    eprintln!("  {}", query.dimmed());
    eprintln!();
    eprintln!(
        "{} Use `uniql validate \"...\"` to check query syntax.",
        "Hint:".yellow().bold()
    );
}
