use anyhow::{bail, Result};

use super::{
    ast::{AstNode, CaseArm, Redirect, RedirectKind, SimpleCommand},
    lexer::{RedirectOp, Token},
};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<AstNode> {
        let node = self.parse_list(&[])?;
        self.skip_semi();
        if !matches!(self.peek(), Token::Eof) {
            bail!("token inesperado: {:?}", self.peek());
        }
        Ok(node)
    }

    fn parse_list(&mut self, stops: &[&str]) -> Result<AstNode> {
        let mut nodes = Vec::new();
        self.skip_semi();

        while !matches!(self.peek(), Token::Eof | Token::RBrace | Token::RParen | Token::DblSemi)
            && !self.is_stop(stops)
        {
            nodes.push(self.parse_and_or()?);
            self.skip_semi();
        }

        Ok(match nodes.len() {
            0 => AstNode::Empty,
            1 => nodes.remove(0),
            _ => AstNode::Sequence(nodes),
        })
    }

    fn parse_and_or(&mut self) -> Result<AstNode> {
        let mut node = self.parse_pipeline()?;

        loop {
            node = match self.peek() {
                Token::AndIf => {
                    self.pos += 1;
                    AstNode::And(Box::new(node), Box::new(self.parse_pipeline()?))
                }
                Token::OrIf => {
                    self.pos += 1;
                    AstNode::Or(Box::new(node), Box::new(self.parse_pipeline()?))
                }
                _ => break,
            };
        }

        Ok(node)
    }

    fn parse_pipeline(&mut self) -> Result<AstNode> {
        let mut parts = vec![self.parse_command()?];

        while matches!(self.peek(), Token::Pipe) {
            self.pos += 1;
            self.skip_semi();
            parts.push(self.parse_command()?);
        }

        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            AstNode::Pipeline(parts)
        })
    }

    fn parse_command(&mut self) -> Result<AstNode> {
        match self.peek() {
            Token::Word(word) if word == "if" => self.parse_if(),
            Token::Word(word) if word == "for" => self.parse_for(),
            Token::Word(word) if word == "while" || word == "until" => self.parse_while(),
            Token::Word(word) if word == "case" => self.parse_case(),
            Token::Word(word) if word == "[[" => self.parse_conditional(),
            Token::LParen if self.tokens.get(self.pos + 1) == Some(&Token::LParen) => {
                self.parse_arithmetic_command()
            },
            Token::LParen => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RParen)?;
                Ok(AstNode::Subshell(Box::new(body)))
            }
            Token::LBrace => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RBrace)?;
                Ok(AstNode::Group(Box::new(body)))
            }
            Token::Word(name)
                if self.tokens.get(self.pos + 1) == Some(&Token::LParen)
                    && self.tokens.get(self.pos + 2) == Some(&Token::RParen) =>
            {
                let name = name.clone();
                self.pos += 3;
                self.skip_semi();
                self.expect_token(Token::LBrace)?;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RBrace)?;
                Ok(AstNode::FunctionDef {
                    name,
                    body: Box::new(body),
                })
            }
            _ => self.parse_simple(),
        }
    }

    fn parse_if(&mut self) -> Result<AstNode> {
        self.expect_word("if")?;
        let condition = self.parse_list(&["then"])?;
        self.expect_word("then")?;
        self.skip_semi();

        let then_branch = self.parse_list(&["else", "elif", "fi"])?;
        let else_branch = if self.word_is("else") {
            self.pos += 1;
            self.skip_semi();
            Some(Box::new(self.parse_list(&["fi"])?))
        } else if self.word_is("elif") {
            self.tokens[self.pos] = Token::Word("if".into());
            Some(Box::new(self.parse_if()?))
        } else {
            None
        };

        self.expect_word("fi")?;
        Ok(AstNode::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
        })
    }

    fn parse_for(&mut self) -> Result<AstNode> {
        self.expect_word("for")?;
        let name = self.take_word()?;
        let mut words = Vec::new();

        if self.word_is("in") {
            self.pos += 1;
            while !matches!(self.peek(), Token::Semi | Token::Eof) {
                words.push(self.take_word()?);
            }
        }

        self.skip_semi();
        self.expect_word("do")?;
        self.skip_semi();
        let body = self.parse_list(&["done"])?;
        self.expect_word("done")?;

        Ok(AstNode::For {
            name,
            words,
            body: Box::new(body),
        })
    }

    fn parse_while(&mut self) -> Result<AstNode> {
        let until = self.word_is("until");
        self.pos += 1;

        let condition = self.parse_list(&["do"])?;
        self.expect_word("do")?;
        self.skip_semi();
        let body = self.parse_list(&["done"])?;
        self.expect_word("done")?;

        Ok(AstNode::While {
            condition: Box::new(condition),
            body: Box::new(body),
            until,
        })
    }

    fn parse_case(&mut self) -> Result<AstNode> {
        self.expect_word("case")?;
        let word = self.take_word()?;
        self.expect_word("in")?;
        self.skip_semi();

        let mut arms = Vec::new();

        while !self.word_is("esac") {
            while matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            if self.word_is("esac") {
                break;
            }

            let mut patterns = Vec::new();
            loop {
                match self.peek().clone() {
                    Token::Word(pattern) => {
                        self.pos += 1;
                        patterns.push(pattern);
                    }
                    Token::Pipe => {
                        self.pos += 1;
                    }
                    Token::RParen => {
                        self.pos += 1;
                        break;
                    }
                    other => bail!("case: patrón inválido: {other:?}"),
                }
            }

            if patterns.is_empty() {
                bail!("case: brazo sin patrón");
            }

            let body = self.parse_list(&["esac"])?;
            arms.push(CaseArm {
                patterns,
                body: Box::new(body),
            });

            if matches!(self.peek(), Token::DblSemi) {
                self.pos += 1;
            } else if !self.word_is("esac") {
                bail!("case: se esperaba ';;' o 'esac'");
            }
            self.skip_semi();
        }

        self.expect_word("esac")?;
        Ok(AstNode::Case { word, arms })
    }

    fn parse_conditional(&mut self) -> Result<AstNode> {
        self.expect_word("[[")?;
        let mut expression = Vec::new();

        while !self.word_is("]]") {
            let token = self.peek().clone();
            match token {
                Token::Eof => bail!("[[: falta ']]'"),
                Token::Word(word) => {
                    self.pos += 1;
                    expression.push(word);
                }
                Token::AndIf => {
                    self.pos += 1;
                    expression.push("&&".to_owned());
                }
                Token::OrIf => {
                    self.pos += 1;
                    expression.push("||".to_owned());
                }
                Token::LParen => {
                    self.pos += 1;
                    expression.push("(".to_owned());
                }
                Token::RParen => {
                    self.pos += 1;
                    expression.push(")".to_owned());
                }
                Token::Redirect { op: RedirectOp::Write, .. } => {
                    self.pos += 1;
                    expression.push(">".to_owned());
                }
                Token::Redirect { op: RedirectOp::Read, .. } => {
                    self.pos += 1;
                    expression.push("<".to_owned());
                }
                other => bail!("[[: token no soportado: {other:?}"),
            }
        }

        self.expect_word("]]")?;
        Ok(AstNode::Conditional(expression))
    }

    fn parse_arithmetic_command(&mut self) -> Result<AstNode> {
        self.expect_token(Token::LParen)?;
        self.expect_token(Token::LParen)?;

        let mut depth = 0usize;
        let mut parts = Vec::new();

        loop {
            match self.peek().clone() {
                Token::Eof => bail!("((: expresión sin cerrar"),
                Token::LParen => {
                    depth += 1;
                    self.pos += 1;
                    parts.push("(".to_owned());
                }
                Token::RParen if depth > 0 => {
                    depth -= 1;
                    self.pos += 1;
                    parts.push(")".to_owned());
                }
                Token::RParen
                    if self.tokens.get(self.pos + 1) == Some(&Token::RParen) =>
                {
                    self.pos += 2;
                    break;
                }
                Token::Word(word) => {
                    self.pos += 1;
                    parts.push(word);
                }
                Token::AndIf => {
                    self.pos += 1;
                    parts.push("&&".to_owned());
                }
                Token::OrIf => {
                    self.pos += 1;
                    parts.push("||".to_owned());
                }
                Token::Pipe => {
                    self.pos += 1;
                    parts.push("|".to_owned());
                }
                Token::Redirect { op: RedirectOp::Write, .. } => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::Word(word) if word == "=") {
                        self.pos += 1;
                        parts.push(">=".to_owned());
                    } else {
                        parts.push(">".to_owned());
                    }
                }
                Token::Redirect { op: RedirectOp::Read, .. } => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::Word(word) if word == "=") {
                        self.pos += 1;
                        parts.push("<=".to_owned());
                    } else {
                        parts.push("<".to_owned());
                    }
                }
                other => bail!("((: token no soportado: {other:?}"),
            }
        }

        Ok(AstNode::ArithmeticCommand(parts.join(" ")))
    }

    fn parse_simple(&mut self) -> Result<AstNode> {
        let mut command = SimpleCommand::default();

        loop {
            match self.peek().clone() {
                Token::Word(word) => {
                    self.pos += 1;
                    command.words.push(word);
                }
                Token::Redirect { fd, op } => {
                    self.pos += 1;
                    let target = self.take_word()?;
                    command.redirects.push(Redirect {
                        fd,
                        kind: match op {
                            RedirectOp::Read => RedirectKind::Read,
                            RedirectOp::Write => RedirectKind::Write,
                            RedirectOp::Append => RedirectKind::Append,
                            RedirectOp::Dup => RedirectKind::Dup,
                            RedirectOp::HereString => RedirectKind::HereString,
                        },
                        target,
                    });
                }
                _ => break,
            }
        }

        if command.words.is_empty() && command.redirects.is_empty() {
            bail!("se esperaba un comando");
        }

        Ok(AstNode::Simple(command))
    }

    fn skip_semi(&mut self) {
        while matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
    }

    fn is_stop(&self, stops: &[&str]) -> bool {
        matches!(self.peek(), Token::Word(word) if stops.contains(&word.as_str()))
    }

    fn word_is(&self, expected: &str) -> bool {
        matches!(self.peek(), Token::Word(word) if word == expected)
    }

    fn expect_word(&mut self, expected: &str) -> Result<()> {
        if self.word_is(expected) {
            self.pos += 1;
            Ok(())
        } else {
            bail!("se esperaba '{expected}', se obtuvo {:?}", self.peek())
        }
    }

    fn take_word(&mut self) -> Result<String> {
        match self.peek().clone() {
            Token::Word(word) => {
                self.pos += 1;
                Ok(word)
            }
            other => bail!("se esperaba una palabra, se obtuvo {other:?}"),
        }
    }

    fn expect_token(&mut self, expected: Token) -> Result<()> {
        if self.peek() == &expected {
            self.pos += 1;
            Ok(())
        } else {
            bail!("se esperaba {expected:?}, se obtuvo {:?}", self.peek())
        }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::bash::lexer::lex;

    #[test]
    fn parses_control_flow_and_functions() {
        assert!(matches!(
            Parser::new(lex("if true; then echo ok; fi").unwrap()).parse().unwrap(),
            AstNode::If { .. }
        ));
        assert!(matches!(
            Parser::new(lex("for x in a b; do echo $x; done").unwrap()).parse().unwrap(),
            AstNode::For { .. }
        ));
        assert!(matches!(
            Parser::new(lex("f() { echo hi; }").unwrap()).parse().unwrap(),
            AstNode::FunctionDef { .. }
        ));
        assert!(matches!(
            Parser::new(lex("case $x in a|b) echo yes ;; *) echo no ;; esac").unwrap()).parse().unwrap(),
            AstNode::Case { .. }
        ));
        assert!(matches!(
            Parser::new(lex("[[ -n $x && $x == ok ]]").unwrap()).parse().unwrap(),
            AstNode::Conditional(_)
        ));
        assert!(matches!(
            Parser::new(lex("(( 1 + 2 ))").unwrap()).parse().unwrap(),
            AstNode::ArithmeticCommand(_)
        ));
    }
}
