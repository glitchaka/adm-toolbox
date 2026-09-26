pub mod ast;
pub mod environment;
pub mod interpreter;
pub mod lexer;
pub mod parser;

pub use environment::ShellEnvironment;
pub use interpreter::{ExecutionResult, Interpreter, ShellCommandHost};

use anyhow::Result;
use ast::AstNode;

pub fn parse(input: &str) -> Result<AstNode> {
    parser::Parser::new(lexer::lex(input)?).parse()
}
