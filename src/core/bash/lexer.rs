use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Pipe,
    AndIf,
    OrIf,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Redirect { fd: i32, op: RedirectOp },
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectOp {
    Read,
    Write,
    Append,
    Dup,
    HereString,
}

pub fn lex(input: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut word = String::new();
    let mut i = 0usize;
    let mut single = false;
    let mut double = false;
    let mut escaped = false;

    fn flush(word: &mut String, out: &mut Vec<Token>) {
        if !word.is_empty() {
            out.push(Token::Word(std::mem::take(word)));
        }
    }

    while i < chars.len() {
        let ch = chars[i];

        if escaped {
            word.push(ch);
            escaped = false;
            i += 1;
            continue;
        }

        if single {
            word.push(ch);
            if ch == '\'' {
                single = false;
            }
            i += 1;
            continue;
        }

        if double {
            word.push(ch);
            if ch == '\\' && i + 1 < chars.len() {
                word.push(ch);
                escaped = true;
            } else if ch == '"' {
                double = false;
            }
            i += 1;
            continue;
        }

        match ch {
            '$' if chars.get(i + 1) == Some(&'(') => {
                // Keep nested command/arithmetic substitutions in the same word.
                let start = i;
                i += 2;
                let mut depth = 1;
                let mut quote = None;
                while i < chars.len() && depth > 0 {
                    let current = chars[i];
                    if current == '\\' { i = (i + 2).min(chars.len()); continue; }
                    if let Some(q) = quote {
                        if current == q { quote = None; }
                    } else {
                        match current {
                            '\'' | '"' => quote = Some(current),
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {},
                        }
                    }
                    i += 1;
                }
                if depth != 0 { bail!("sustitución sin cerrar"); }
                word.extend(&chars[start..i]);
            }
            '\'' => { word.push(ch); single = true; i += 1; }
            '"' => { word.push(ch); double = true; i += 1; }
            '\\' => { word.push(ch); escaped = true; i += 1; }
            '#' if word.is_empty() => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' | '\r' => { flush(&mut word, &mut out); i += 1; }
            '\n' | ';' => { flush(&mut word, &mut out); out.push(Token::Semi); i += 1; }
            '&' if chars.get(i + 1) == Some(&'&') => {
                flush(&mut word, &mut out);
                out.push(Token::AndIf);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                flush(&mut word, &mut out);
                out.push(Token::OrIf);
                i += 2;
            }
            '|' => { flush(&mut word, &mut out); out.push(Token::Pipe); i += 1; }
            '(' => { flush(&mut word, &mut out); out.push(Token::LParen); i += 1; }
            ')' => { flush(&mut word, &mut out); out.push(Token::RParen); i += 1; }
            '{' if word.is_empty() => { out.push(Token::LBrace); i += 1; }
            '}' if word.is_empty() => { out.push(Token::RBrace); i += 1; }
            '0'..='9'
                if word.is_empty()
                    && chars.get(i + 1).is_some_and(|c| *c == '>' || *c == '<') =>
            {
                let fd = ch.to_digit(10).unwrap_or(1) as i32;
                i += 1;
                let (op, used) = redirect_op(&chars, i)?;
                out.push(Token::Redirect { fd, op });
                i += used;
            }
            '>' | '<' => {
                flush(&mut word, &mut out);
                let (op, used) = redirect_op(&chars, i)?;
                out.push(Token::Redirect {
                    fd: if ch == '<' { 0 } else { 1 },
                    op,
                });
                i += used;
            }
            _ => { word.push(ch); i += 1; }
        }
    }

    if single || double {
        bail!("comillas sin cerrar");
    }

    flush(&mut word, &mut out);
    out.push(Token::Eof);
    Ok(out)
}

fn redirect_op(chars: &[char], i: usize) -> Result<(RedirectOp, usize)> {
    match chars.get(i) {
        Some('>') if chars.get(i + 1) == Some(&'>') => Ok((RedirectOp::Append, 2)),
        Some('>') if chars.get(i + 1) == Some(&'&') => Ok((RedirectOp::Dup, 2)),
        Some('>') => Ok((RedirectOp::Write, 1)),
        Some('<')
            if chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'<') =>
        {
            Ok((RedirectOp::HereString, 3))
        }
        Some('<') => Ok((RedirectOp::Read, 1)),
        _ => bail!("redirección inválida"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_shell_operators_without_losing_quotes() {
        let tokens = lex("echo \"a b\" | grep a && echo ok 2>&1").unwrap();
        assert!(tokens.contains(&Token::Pipe));
        assert!(tokens.contains(&Token::AndIf));
        assert!(tokens.contains(&Token::Redirect { fd: 2, op: RedirectOp::Dup }));
        assert!(tokens.contains(&Token::Word("\"a b\"".into())));
    }
}
