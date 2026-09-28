use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Arithmetic(String),
    Pipe,
    PipeBoth,
    AndIf,
    OrIf,
    Amp,
    Semi,
    DblSemi,
    SemiAmp,
    DblSemiAmp,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Redirect { fd: i32, variable: Option<String>, op: RedirectOp },
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectOp {
    Read,
    Write,
    Append,
    DupInput,
    DupOutput,
    HereString,
    ReadWrite,
    Clobber,
    BothWrite,
    BothAppend,
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
            if ch == '\'' { single = false; }
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
            '(' if word.is_empty() && chars.get(i + 1) == Some(&'(') => {
                flush(&mut word, &mut out);
                i += 2;
                let start = i;
                let mut depth = 0usize;
                let mut quote = None;
                let mut escaped_inner = false;
                let mut closed = false;
                while i < chars.len() {
                    let current = chars[i];
                    if escaped_inner {
                        escaped_inner = false;
                        i += 1;
                        continue;
                    }
                    if current == '\\' {
                        escaped_inner = true;
                        i += 1;
                        continue;
                    }
                    if let Some(q) = quote {
                        if current == q { quote = None; }
                        i += 1;
                        continue;
                    }
                    if matches!(current, '\'' | '"') {
                        quote = Some(current);
                        i += 1;
                        continue;
                    }
                    if current == '(' {
                        depth += 1;
                        i += 1;
                        continue;
                    }
                    if current == ')' {
                        if depth > 0 {
                            depth -= 1;
                            i += 1;
                            continue;
                        }
                        if chars.get(i + 1) == Some(&')') {
                            let expression: String = chars[start..i].iter().collect();
                            out.push(Token::Arithmetic(expression));
                            i += 2;
                            closed = true;
                            break;
                        }
                    }
                    i += 1;
                }
                if !closed {
                    bail!("expresión aritmética sin cerrar");
                }
            }
            '<' | '>' if chars.get(i + 1) == Some(&'(') => {
                // Process substitution is a word-like expansion, not a redirection
                // operator. Preserve the complete construct for the Bash expander.
                let start = i;
                i += 2;
                let mut depth = 1usize;
                let mut quote = None;
                while i < chars.len() && depth > 0 {
                    let current = chars[i];
                    if current == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if let Some(q) = quote {
                        if current == q { quote = None; }
                    } else {
                        match current {
                            '\'' | '"' => quote = Some(current),
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                    }
                    i += 1;
                }
                if depth != 0 { bail!("sustitución de proceso sin cerrar"); }
                word.extend(&chars[start..i]);
            }
            '$' if chars.get(i + 1) == Some(&'{') => {
                // Preserve the entire braced expansion as one word. Bash 5.3
                // permits current-shell command substitutions such as
                // ${ command; } and ${| REPLY=value; }, both of which may
                // contain whitespace and shell operators.
                let start = i;
                i += 2;
                let mut depth = 1usize;
                let mut quote = None;
                while i < chars.len() && depth > 0 {
                    let current = chars[i];
                    if current == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if let Some(q) = quote {
                        if current == q {
                            quote = None;
                        }
                    } else {
                        match current {
                            '\'' | '"' => quote = Some(current),
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                    i += 1;
                }
                if depth != 0 {
                    bail!("expansión con llaves sin cerrar");
                }
                word.extend(&chars[start..i]);
            }
            '$' if chars.get(i + 1) == Some(&'(') => {
                let start = i;
                i += 2;
                let mut depth = 1usize;
                let mut quote = None;
                while i < chars.len() && depth > 0 {
                    let current = chars[i];
                    if current == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if let Some(q) = quote {
                        if current == q { quote = None; }
                    } else {
                        match current {
                            '\'' | '"' => quote = Some(current),
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                    }
                    i += 1;
                }
                if depth != 0 { bail!("sustitución sin cerrar"); }
                word.extend(&chars[start..i]);
            }
            '?' | '*' | '+' | '@' | '!' if chars.get(i + 1) == Some(&'(') => {
                let start = i;
                i += 2;
                let mut depth = 1usize;
                let mut quote = None;
                while i < chars.len() && depth > 0 {
                    let current = chars[i];
                    if current == '\\' {
                        i = (i + 2).min(chars.len());
                        continue;
                    }
                    if let Some(q) = quote {
                        if current == q { quote = None; }
                    } else {
                        match current {
                            '\'' | '"' => quote = Some(current),
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                    }
                    i += 1;
                }
                if depth != 0 { bail!("extglob sin cerrar"); }
                word.extend(&chars[start..i]);
            }
            '$' if chars.get(i + 1) == Some(&'\'') => {
                // ANSI-C quoting: preserve it as part of the word; expansion removes it.
                word.push('$');
                word.push('\'');
                i += 2;
                while i < chars.len() {
                    let current = chars[i];
                    word.push(current);
                    i += 1;
                    if current == '\\' && i < chars.len() {
                        word.push(chars[i]);
                        i += 1;
                        continue;
                    }
                    if current == '\'' { break; }
                }
            }
            '\'' => { word.push(ch); single = true; i += 1; }
            '"' => { word.push(ch); double = true; i += 1; }
            '\\' => { word.push(ch); escaped = true; i += 1; }
            '#' if word.is_empty() => {
                while i < chars.len() && chars[i] != '\n' { i += 1; }
            }
            ' ' | '\t' | '\r' => { flush(&mut word, &mut out); i += 1; }
            '\n' => { flush(&mut word, &mut out); out.push(Token::Semi); i += 1; }
            ';' if chars.get(i + 1) == Some(&';') && chars.get(i + 2) == Some(&'&') => {
                flush(&mut word, &mut out);
                out.push(Token::DblSemiAmp);
                i += 3;
            }
            ';' if chars.get(i + 1) == Some(&'&') => {
                flush(&mut word, &mut out);
                out.push(Token::SemiAmp);
                i += 2;
            }
            ';' if chars.get(i + 1) == Some(&';') => {
                flush(&mut word, &mut out);
                out.push(Token::DblSemi);
                i += 2;
            }
            ';' => { flush(&mut word, &mut out); out.push(Token::Semi); i += 1; }
            '&' if chars.get(i + 1) == Some(&'&') => {
                flush(&mut word, &mut out);
                out.push(Token::AndIf);
                i += 2;
            }
            '&' if chars.get(i + 1) == Some(&'>') && chars.get(i + 2) == Some(&'>') => {
                flush(&mut word, &mut out);
                out.push(Token::Redirect { fd: 1, variable: None, op: RedirectOp::BothAppend });
                i += 3;
            }
            '&' if chars.get(i + 1) == Some(&'>') => {
                flush(&mut word, &mut out);
                out.push(Token::Redirect { fd: 1, variable: None, op: RedirectOp::BothWrite });
                i += 2;
            }
            '&' => { flush(&mut word, &mut out); out.push(Token::Amp); i += 1; }
            '|' if chars.get(i + 1) == Some(&'|') => {
                flush(&mut word, &mut out);
                out.push(Token::OrIf);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'&') => {
                flush(&mut word, &mut out);
                out.push(Token::PipeBoth);
                i += 2;
            }
            '|' => { flush(&mut word, &mut out); out.push(Token::Pipe); i += 1; }
            '(' => { flush(&mut word, &mut out); out.push(Token::LParen); i += 1; }
            ')' => { flush(&mut word, &mut out); out.push(Token::RParen); i += 1; }
            '{' if word.is_empty() => {
                let mut end = i + 1;
                while end < chars.len() && (chars[end] == '_' || chars[end].is_ascii_alphanumeric()) {
                    end += 1;
                }
                let variable_redirect = end > i + 1
                    && chars.get(end) == Some(&'}')
                    && chars.get(end + 1).is_some_and(|ch| matches!(ch, '>' | '<'));
                if variable_redirect {
                    let name: String = chars[i + 1..end].iter().collect();
                    let valid = name.chars().next().is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
                        && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric());
                    if valid {
                        let op_index = end + 1;
                        let (op, used) = redirect_op(&chars, op_index)?;
                        out.push(Token::Redirect { fd: -1, variable: Some(name), op });
                        i = op_index + used;
                    } else {
                        out.push(Token::LBrace);
                        i += 1;
                    }
                } else {
                    out.push(Token::LBrace);
                    i += 1;
                }
            }
            '}' if word.is_empty() => { out.push(Token::RBrace); i += 1; }
            '0'..='9' if word.is_empty() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && matches!(chars[i], '>' | '<') {
                    let fd_text: String = chars[start..i].iter().collect();
                    let fd = fd_text.parse::<i32>().map_err(|_| anyhow::anyhow!("descriptor inválido: {fd_text}"))?;
                    let (op, used) = redirect_op(&chars, i)?;
                    out.push(Token::Redirect { fd, variable: None, op });
                    i += used;
                } else {
                    word.extend(&chars[start..i]);
                }
            }
            '>' | '<' => {
                flush(&mut word, &mut out);
                let (op, used) = redirect_op(&chars, i)?;
                out.push(Token::Redirect { fd: if ch == '<' { 0 } else { 1 }, variable: None, op });
                i += used;
            }
            _ => { word.push(ch); i += 1; }
        }
    }

    if single || double { bail!("comillas sin cerrar"); }
    flush(&mut word, &mut out);
    out.push(Token::Eof);
    Ok(out)
}

fn redirect_op(chars: &[char], i: usize) -> Result<(RedirectOp, usize)> {
    match chars.get(i) {
        Some('>') if chars.get(i + 1) == Some(&'>') => Ok((RedirectOp::Append, 2)),
        Some('>') if chars.get(i + 1) == Some(&'&') => Ok((RedirectOp::DupOutput, 2)),
        Some('>') if chars.get(i + 1) == Some(&'|') => Ok((RedirectOp::Clobber, 2)),
        Some('>') => Ok((RedirectOp::Write, 1)),
        Some('<') if chars.get(i + 1) == Some(&'>') => Ok((RedirectOp::ReadWrite, 2)),
        Some('<') if chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'<') => {
            Ok((RedirectOp::HereString, 3))
        }
        Some('<') if chars.get(i + 1) == Some(&'&') => Ok((RedirectOp::DupInput, 2)),
        Some('<') => Ok((RedirectOp::Read, 1)),
        _ => bail!("redirección inválida"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_shell_operators_without_losing_quotes() {
        let tokens = lex("echo \"a b\" | grep a && echo ok 2>&1 &").unwrap();
        assert!(tokens.contains(&Token::Pipe));
        assert!(tokens.contains(&Token::AndIf));
        assert!(tokens.contains(&Token::Amp));
        assert!(tokens.contains(&Token::Redirect { fd: 2, variable: None, op: RedirectOp::DupOutput }));
        assert!(tokens.contains(&Token::Word("\"a b\"".into())));
    }
}
