use anyhow::{bail, Result};

use super::{
    ast::{AstNode, CaseArm, CaseTerminator, Redirect, RedirectKind, SimpleCommand},
    lexer::{RedirectOp, Token},
};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }

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

        while !matches!(
            self.peek(),
            Token::Eof | Token::RBrace | Token::RParen | Token::DblSemi | Token::SemiAmp | Token::DblSemiAmp
        ) && !self.is_stop(stops)
        {
            let mut node = self.parse_and_or()?;
            if matches!(self.peek(), Token::Amp) {
                self.pos += 1;
                node = AstNode::Background(Box::new(node));
            }
            nodes.push(node);
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
        let timed = self.word_is("time");
        let mut posix_time = false;
        if timed {
            self.pos += 1;
            if self.word_is("-p") {
                posix_time = true;
                self.pos += 1;
            }
        }

        let negate = self.word_is("!");
        if negate { self.pos += 1; }

        let mut parts = vec![self.parse_command()?];
        let mut stderr_to_pipe = Vec::new();
        while matches!(self.peek(), Token::Pipe | Token::PipeBoth) {
            let merge_stderr = matches!(self.peek(), Token::PipeBoth);
            self.pos += 1;
            self.skip_semi();
            stderr_to_pipe.push(merge_stderr);
            parts.push(self.parse_command()?);
        }

        let mut node = if parts.len() == 1 {
            parts.remove(0)
        } else {
            AstNode::Pipeline { parts, stderr_to_pipe }
        };
        if negate {
            node = AstNode::Negate(Box::new(node));
        }
        if timed {
            node = AstNode::Time { body: Box::new(node), posix: posix_time };
        }
        Ok(node)
    }

    fn parse_command(&mut self) -> Result<AstNode> {
        let mut node = match self.peek() {
            Token::Arithmetic(expression) => {
                let expression = expression.clone();
                self.pos += 1;
                AstNode::ArithmeticCommand(expression)
            }
            Token::Word(word) if word == "coproc" => self.parse_coproc()?,
            Token::Word(word) if word == "if" => self.parse_if()?,
            Token::Word(word) if word == "for" => self.parse_for()?,
            Token::Word(word) if word == "select" => self.parse_select()?,
            Token::Word(word) if word == "while" || word == "until" => self.parse_while()?,
            Token::Word(word) if word == "case" => self.parse_case()?,
            Token::Word(word) if word == "[[" => self.parse_conditional()?,
            Token::Word(word) if word == "function" => self.parse_function_keyword()?,
            Token::LParen if self.tokens.get(self.pos + 1) == Some(&Token::LParen) => {
                self.parse_arithmetic_command()?
            }
            Token::LParen => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RParen)?;
                AstNode::Subshell(Box::new(body))
            }
            Token::LBrace => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RBrace)?;
                AstNode::Group(Box::new(body))
            }
            Token::Word(name)
                if self.tokens.get(self.pos + 1) == Some(&Token::LParen)
                    && self.tokens.get(self.pos + 2) == Some(&Token::RParen) =>
            {
                let name = name.clone();
                self.pos += 3;
                self.skip_semi();
                let body = self.parse_function_body()?;
                AstNode::FunctionDef { name, body: Box::new(body) }
            }
            Token::Word(word)
                if word.ends_with('=')
                    && self.tokens.get(self.pos + 1) == Some(&Token::LParen) =>
            {
                self.parse_array_assignment()?
            }
            _ => return self.parse_simple(),
        };

        let redirects = self.parse_trailing_redirects()?;
        if !redirects.is_empty() {
            node = AstNode::Redirected {
                body: Box::new(node),
                redirects,
            };
        }
        Ok(node)
    }

    fn parse_trailing_redirects(&mut self) -> Result<Vec<Redirect>> {
        let mut redirects = Vec::new();
        while let Token::Redirect { fd, op } = self.peek().clone() {
            self.pos += 1;
            let target = self.take_word()?;
            redirects.push(Redirect {
                fd,
                kind: match op {
                    RedirectOp::Read => RedirectKind::Read,
                    RedirectOp::Write => RedirectKind::Write,
                    RedirectOp::Append => RedirectKind::Append,
                    RedirectOp::DupInput => RedirectKind::DupInput,
                    RedirectOp::DupOutput => RedirectKind::DupOutput,
                    RedirectOp::HereString => RedirectKind::HereString,
                    RedirectOp::ReadWrite => RedirectKind::ReadWrite,
                    RedirectOp::Clobber => RedirectKind::Clobber,
                    RedirectOp::BothWrite => RedirectKind::BothWrite,
                    RedirectOp::BothAppend => RedirectKind::BothAppend,
                },
                target,
            });
        }
        Ok(redirects)
    }

    fn parse_coproc(&mut self) -> Result<AstNode> {
        self.expect_word("coproc")?;

        let mut name = None;
        if let Token::Word(candidate) = self.peek().clone() {
            let next_is_compound = self.tokens.get(self.pos + 1).is_some_and(|token| {
                matches!(token, Token::LBrace | Token::LParen | Token::Arithmetic(_))
                    || matches!(token,
                        Token::Word(word)
                        if matches!(word.as_str(),
                            "if" | "for" | "select" | "while" | "until" | "case"
                            | "function" | "[["
                        )
                    )
            });
            if next_is_compound && is_shell_name(&candidate) {
                name = Some(candidate);
                self.pos += 1;
            }
        }

        let body = self.parse_command()?;
        Ok(AstNode::Coproc {
            name,
            body: Box::new(body),
        })
    }

    fn parse_function_body(&mut self) -> Result<AstNode> {
        match self.peek() {
            Token::LBrace => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RBrace)?;
                Ok(body)
            }
            Token::LParen => {
                self.pos += 1;
                let body = self.parse_list(&[])?;
                self.expect_token(Token::RParen)?;
                Ok(AstNode::Subshell(Box::new(body)))
            }
            _ => self.parse_command(),
        }
    }

    fn parse_function_keyword(&mut self) -> Result<AstNode> {
        self.expect_word("function")?;
        let name = self.take_word()?;
        if matches!(self.peek(), Token::LParen)
            && self.tokens.get(self.pos + 1) == Some(&Token::RParen)
        {
            self.pos += 2;
        }
        self.skip_semi();
        let body = self.parse_function_body()?;
        Ok(AstNode::FunctionDef { name, body: Box::new(body) })
    }

    fn parse_array_assignment(&mut self) -> Result<AstNode> {
        let assignment = self.take_word()?;
        let name = assignment.trim_end_matches('=').to_owned();
        self.expect_token(Token::LParen)?;
        let mut words = Vec::new();
        while !matches!(self.peek(), Token::RParen | Token::Eof) {
            match self.peek().clone() {
                Token::Word(word) => { self.pos += 1; words.push(word); }
                Token::Semi => { self.pos += 1; }
                other => bail!("asignación de array: token inesperado {other:?}"),
            }
        }
        self.expect_token(Token::RParen)?;
        Ok(AstNode::ArrayAssign { name, words })
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
        if let Token::Arithmetic(expression) = self.peek().clone() {
            self.pos += 1;
            let sections = split_arithmetic_for_sections(&expression)?;
            self.skip_semi();
            self.expect_word("do")?;
            self.skip_semi();
            let body = self.parse_list(&["done"])?;
            self.expect_word("done")?;
            return Ok(AstNode::ArithmeticFor {
                init: sections[0].clone(),
                condition: sections[1].clone(),
                update: sections[2].clone(),
                body: Box::new(body),
            });
        }

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
        Ok(AstNode::For { name, words, body: Box::new(body) })
    }

    fn parse_select(&mut self) -> Result<AstNode> {
        self.expect_word("select")?;
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
        Ok(AstNode::Select { name, words, body: Box::new(body) })
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
            while matches!(self.peek(), Token::Semi) { self.pos += 1; }
            if self.word_is("esac") { break; }

            if matches!(self.peek(), Token::LParen) { self.pos += 1; }
            let mut patterns = Vec::new();
            loop {
                match self.peek().clone() {
                    Token::Word(pattern) => { self.pos += 1; patterns.push(pattern); }
                    Token::Pipe => { self.pos += 1; }
                    Token::RParen => { self.pos += 1; break; }
                    other => bail!("case: patrón inválido: {other:?}"),
                }
            }
            if patterns.is_empty() { bail!("case: brazo sin patrón"); }

            let body = self.parse_list(&["esac"])?;
            let terminator = match self.peek() {
                Token::DblSemi => { self.pos += 1; CaseTerminator::Break }
                Token::SemiAmp => { self.pos += 1; CaseTerminator::Fallthrough }
                Token::DblSemiAmp => { self.pos += 1; CaseTerminator::ContinueMatching }
                _ if self.word_is("esac") => CaseTerminator::Break,
                _ => bail!("case: se esperaba ';;', ';&', ';;&' o 'esac'"),
            };
            arms.push(CaseArm { patterns, body: Box::new(body), terminator });
            self.skip_semi();
        }

        self.expect_word("esac")?;
        Ok(AstNode::Case { word, arms })
    }

    fn parse_conditional(&mut self) -> Result<AstNode> {
        self.expect_word("[[")?;
        let mut expression = Vec::new();
        while !self.word_is("]]") {
            match self.peek().clone() {
                Token::Eof => bail!("[[: falta ']]'"),
                Token::Word(word) => { self.pos += 1; expression.push(word); }
                Token::AndIf => { self.pos += 1; expression.push("&&".to_owned()); }
                Token::OrIf => { self.pos += 1; expression.push("||".to_owned()); }
                Token::LParen => { self.pos += 1; expression.push("(".to_owned()); }
                Token::RParen => { self.pos += 1; expression.push(")".to_owned()); }
                Token::Redirect { op: RedirectOp::Write, .. } => { self.pos += 1; expression.push(">".to_owned()); }
                Token::Redirect { op: RedirectOp::Read, .. } => { self.pos += 1; expression.push("<".to_owned()); }
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
                Token::LParen => { depth += 1; self.pos += 1; parts.push("(".to_owned()); }
                Token::RParen if depth > 0 => { depth -= 1; self.pos += 1; parts.push(")".to_owned()); }
                Token::RParen if self.tokens.get(self.pos + 1) == Some(&Token::RParen) => {
                    self.pos += 2;
                    break;
                }
                Token::Word(word) => { self.pos += 1; parts.push(word); }
                Token::Arithmetic(expression) => { self.pos += 1; parts.push(expression); }
                Token::AndIf => { self.pos += 1; parts.push("&&".to_owned()); }
                Token::OrIf => { self.pos += 1; parts.push("||".to_owned()); }
                Token::Pipe => { self.pos += 1; parts.push("|".to_owned()); }
                Token::Redirect { op: RedirectOp::Write, .. } => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::Word(word) if word == "=") {
                        self.pos += 1; parts.push(">=".to_owned());
                    } else { parts.push(">".to_owned()); }
                }
                Token::Redirect { op: RedirectOp::Read, .. } => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::Word(word) if word == "=") {
                        self.pos += 1; parts.push("<=".to_owned());
                    } else { parts.push("<".to_owned()); }
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
                Token::Word(word) => { self.pos += 1; command.words.push(word); }
                Token::LParen if command.words.last().is_some_and(|word| word.ends_with('=')) => {
                    self.pos += 1;
                    let mut literal = String::from("(");
                    let mut depth = 1usize;
                    while depth > 0 {
                        match self.peek().clone() {
                            Token::Eof => bail!("array: literal sin cerrar"),
                            Token::LParen => {
                                depth += 1;
                                literal.push('(');
                                self.pos += 1;
                            }
                            Token::RParen => {
                                depth -= 1;
                                self.pos += 1;
                                if depth > 0 { literal.push(')'); }
                            }
                            Token::Word(word) => {
                                if !literal.ends_with('(') { literal.push(' '); }
                                literal.push_str(&word);
                                self.pos += 1;
                            }
                            Token::Semi => {
                                literal.push(' ');
                                self.pos += 1;
                            }
                            other => bail!("array: token inesperado {other:?}"),
                        }
                    }
                    literal.push(')');
                    if let Some(last) = command.words.last_mut() {
                        last.push_str(&literal);
                    }
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
                            RedirectOp::DupInput => RedirectKind::DupInput,
                            RedirectOp::DupOutput => RedirectKind::DupOutput,
                            RedirectOp::HereString => RedirectKind::HereString,
                            RedirectOp::ReadWrite => RedirectKind::ReadWrite,
                            RedirectOp::Clobber => RedirectKind::Clobber,
                            RedirectOp::BothWrite => RedirectKind::BothWrite,
                            RedirectOp::BothAppend => RedirectKind::BothAppend,
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
        while matches!(self.peek(), Token::Semi) { self.pos += 1; }
    }

    fn is_stop(&self, stops: &[&str]) -> bool {
        matches!(self.peek(), Token::Word(word) if stops.contains(&word.as_str()))
    }

    fn word_is(&self, expected: &str) -> bool {
        matches!(self.peek(), Token::Word(word) if word == expected)
    }

    fn expect_word(&mut self, expected: &str) -> Result<()> {
        if self.word_is(expected) { self.pos += 1; Ok(()) }
        else { bail!("se esperaba '{expected}', se obtuvo {:?}", self.peek()) }
    }

    fn take_word(&mut self) -> Result<String> {
        match self.peek().clone() {
            Token::Word(word) => { self.pos += 1; Ok(word) }
            other => bail!("se esperaba una palabra, se obtuvo {other:?}"),
        }
    }

    fn expect_token(&mut self, expected: Token) -> Result<()> {
        if self.peek() == &expected { self.pos += 1; Ok(()) }
        else { bail!("se esperaba {expected:?}, se obtuvo {:?}", self.peek()) }
    }

    fn peek(&self) -> &Token { self.tokens.get(self.pos).unwrap_or(&Token::Eof) }
}


fn is_shell_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else { return false };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn split_arithmetic_for_sections(expression: &str) -> Result<[String; 3]> {
    let mut sections = [String::new(), String::new(), String::new()];
    let mut section = 0usize;
    let mut depth = 0i32;
    let mut single = false;
    let mut double = false;
    let mut escaped = false;

    for ch in expression.chars() {
        if escaped {
            sections[section].push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            sections[section].push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !double {
            single = !single;
            sections[section].push(ch);
            continue;
        }
        if ch == '"' && !single {
            double = !double;
            sections[section].push(ch);
            continue;
        }
        if !single && !double {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' if depth == 0 => {
                    if section >= 2 {
                        bail!("for ((...)): demasiados separadores ';'");
                    }
                    section += 1;
                    continue;
                }
                _ => {}
            }
        }
        sections[section].push(ch);
    }

    if section != 2 {
        bail!("for ((...)): se esperaban tres expresiones separadas por ';'");
    }
    for value in &mut sections {
        *value = value.trim().to_owned();
    }
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::bash::lexer::lex;

    #[test]
    fn parses_extended_control_flow() {
        assert!(matches!(Parser::new(lex("if true; then echo ok; fi").unwrap()).parse().unwrap(), AstNode::If { .. }));
        assert!(matches!(Parser::new(lex("for x in a b; do echo $x; done").unwrap()).parse().unwrap(), AstNode::For { .. }));
        assert!(matches!(Parser::new(lex("select x in a b; do break; done").unwrap()).parse().unwrap(), AstNode::Select { .. }));
        assert!(matches!(Parser::new(lex("f() { echo hi; }").unwrap()).parse().unwrap(), AstNode::FunctionDef { .. }));
        assert!(matches!(Parser::new(lex("case $x in a|b) echo yes ;; *) echo no ;; esac").unwrap()).parse().unwrap(), AstNode::Case { .. }));
        assert!(matches!(Parser::new(lex("[[ -n $x && $x == ok ]]").unwrap()).parse().unwrap(), AstNode::Conditional(_)));
        assert!(matches!(Parser::new(lex("(( 1 + 2 ))").unwrap()).parse().unwrap(), AstNode::ArithmeticCommand(_)));
        assert!(matches!(Parser::new(lex("echo hi &").unwrap()).parse().unwrap(), AstNode::Background(_)));
        assert!(matches!(Parser::new(lex("arr=(a b c)").unwrap()).parse().unwrap(), AstNode::ArrayAssign { .. }));
    }
}
