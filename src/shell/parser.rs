use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug)]
pub enum ChainOp {
    Always,
    And,
    Or,
}

#[derive(Debug)]
pub struct ParsedLine {
    pub segments: Vec<Segment>,
}

#[derive(Debug)]
pub struct Segment {
    pub gate: Option<ChainOp>,
    pub pipeline: Pipeline,
}

#[derive(Debug)]
pub struct Pipeline {
    pub commands: Vec<ParsedCommand>,
    pub redirect: Option<Redirection>,
    pub input_redirect: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub argv: Vec<String>,
}

#[derive(Debug)]
pub struct Redirection {
    pub path: String,
    pub append: bool,
}

#[derive(Debug)]
enum Token {
    Word(String),
    Pipe,
    Redirect,
    Append,
    Input,
    And,
    Or,
    Semicolon,
}

pub fn parse(line: &str) -> Result<ParsedLine> {
    let tokens = tokenize(line)?;
    let mut segments = Vec::new();
    let mut gate = None;
    let mut commands = vec![ParsedCommand { argv: Vec::new() }];
    let mut redirect = None;
    let mut input_redirect = None;
    let mut index = 0;

    while index < tokens.len() {
        match &tokens[index] {
            Token::Word(word) => commands.last_mut().unwrap().argv.push(word.clone()),
            Token::Pipe => {
                if commands.last().is_none_or(|command| command.argv.is_empty()) {
                    bail!("pipe sin comando anterior");
                }
                commands.push(ParsedCommand { argv: Vec::new() });
            }
            Token::Redirect | Token::Append => {
                let append = matches!(tokens[index], Token::Append);
                index += 1;
                let Some(Token::Word(path)) = tokens.get(index) else {
                    bail!("falta destino después de la redirección");
                };
                redirect = Some(Redirection {
                    path: path.clone(),
                    append,
                });
            }
            Token::Input => {
                index += 1;
                let Some(Token::Word(path)) = tokens.get(index) else {
                    bail!("falta archivo después de <");
                };
                input_redirect = Some(path.clone());
            }
            Token::And | Token::Or | Token::Semicolon => {
                push_segment(
                    &mut segments,
                    gate,
                    &mut commands,
                    redirect.take(),
                    input_redirect.take(),
                )?;
                gate = Some(match tokens[index] {
                    Token::And => ChainOp::And,
                    Token::Or => ChainOp::Or,
                    Token::Semicolon => ChainOp::Always,
                    _ => unreachable!(),
                });
                commands = vec![ParsedCommand { argv: Vec::new() }];
            }
        }
        index += 1;
    }

    push_segment(
        &mut segments,
        gate,
        &mut commands,
        redirect,
        input_redirect,
    )?;
    Ok(ParsedLine { segments })
}

fn push_segment(
    segments: &mut Vec<Segment>,
    gate: Option<ChainOp>,
    commands: &mut Vec<ParsedCommand>,
    redirect: Option<Redirection>,
    input_redirect: Option<String>,
) -> Result<()> {
    commands.retain(|command| !command.argv.is_empty());
    if commands.is_empty() {
        return Ok(());
    }

    segments.push(Segment {
        gate,
        pipeline: Pipeline {
            commands: std::mem::take(commands),
            redirect,
            input_redirect,
        },
    });

    Ok(())
}

fn tokenize(line: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }

        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }

        let operator = match ch {
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                Some(Token::Or)
            }
            '|' => Some(Token::Pipe),
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                Some(Token::And)
            }
            '>' if chars.peek() == Some(&'>') => {
                chars.next();
                Some(Token::Append)
            }
            '>' => Some(Token::Redirect),
            '<' => Some(Token::Input),
            ';' => Some(Token::Semicolon),
            _ => None,
        };

        if let Some(operator) = operator {
            flush_word(&mut tokens, &mut word);
            tokens.push(operator);
            continue;
        }

        if ch.is_whitespace() {
            flush_word(&mut tokens, &mut word);
        } else {
            word.push(ch);
        }
    }

    if escaped {
        word.push('\\');
    }

    if quote.is_some() {
        bail!("comillas sin cerrar");
    }

    flush_word(&mut tokens, &mut word);
    Ok(tokens)
}

fn flush_word(tokens: &mut Vec<Token>, word: &mut String) {
    if !word.is_empty() {
        tokens.push(Token::Word(std::mem::take(word)));
    }
}
