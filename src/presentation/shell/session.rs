use std::path::PathBuf;

use anyhow::Result;
use rustyline::{
    Editor,
    error::ReadlineError,
    history::DefaultHistory,
};

use crate::core::ports::ShellEngine;

use super::{
    completion::ShellHelper,
    prompt,
};

pub struct ShellSession {
    editor: Editor<ShellHelper, DefaultHistory>,
    engine: Box<dyn ShellEngine>,
    history_file: PathBuf,
    running: bool,
}

impl ShellSession {
    pub fn new(
        engine: Box<dyn ShellEngine>,
        command_names: Vec<String>,
        history_file: PathBuf,
    ) -> Result<Self> {
        let mut completion_names = command_names;
        completion_names.extend([
            "help".to_owned(),
            "man".to_owned(),
            "config".to_owned(),
            "cd".to_owned(),
            "export".to_owned(),
            "unset".to_owned(),
            "alias".to_owned(),
            "unalias".to_owned(),
            "source".to_owned(),
        ]);

        let mut editor = Editor::<ShellHelper, DefaultHistory>::new()?;
        editor.set_helper(Some(ShellHelper::new(completion_names)));
        let _ = editor.load_history(&history_file);

        Ok(Self {
            editor,
            engine,
            history_file,
            running: true,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        println!("{}", prompt::banner());

        let mut buffer = String::new();

        while self.running {
            let prompt_text = if buffer.is_empty() {
                prompt::render(self.engine.working_dir())
            } else {
                "> ".to_owned()
            };

            match self.editor.readline(&prompt_text) {
                Ok(line) => {
                    if buffer.is_empty() && line.trim().is_empty() {
                        continue;
                    }

                    if !buffer.is_empty() {
                        buffer.push('\n');
                    }
                    buffer.push_str(&line);

                    if needs_continuation(&buffer) {
                        continue;
                    }

                    let command = buffer.trim_end().to_owned();
                    buffer.clear();

                    if command.trim().is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(command.as_str());

                    match self.engine.execute(&command) {
                        Ok(result) => {
                            print!("{}", result.stdout);
                            eprint!("{}", result.stderr);
                            if result.exit_requested {
                                self.running = false;
                            }
                        }
                        Err(error) => eprintln!("adm: {error}"),
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    buffer.clear();
                    println!("^C");
                }
                Err(ReadlineError::Eof) => {
                    println!();
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }

        let _ = self.editor.save_history(&self.history_file);
        Ok(())
    }
}

pub(crate) fn needs_continuation(input: &str) -> bool {
    let trimmed = input.trim_end();

    if trimmed.ends_with('\\')
        || trimmed.ends_with('|')
        || trimmed.ends_with("&&")
        || trimmed.ends_with("||")
    {
        return true;
    }

    let mut single_quote = false;
    let mut double_quote = false;
    let mut backtick = false;
    let mut escaped = false;
    let mut parens = 0_i32;
    let mut braces = 0_i32;
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if escaped {
            escaped = false;
            if !single_quote {
                current.push(ch);
            }
            continue;
        }

        if ch == '\\' && !single_quote {
            escaped = true;
            continue;
        }

        if single_quote {
            if ch == '\'' {
                single_quote = false;
            }
            continue;
        }

        if double_quote {
            if ch == '"' {
                double_quote = false;
            }
            continue;
        }

        if backtick {
            if ch == '`' {
                backtick = false;
            }
            continue;
        }

        match ch {
            '\'' => {
                flush_word(&mut current, &mut words);
                single_quote = true;
            }
            '"' => {
                flush_word(&mut current, &mut words);
                double_quote = true;
            }
            '`' => {
                flush_word(&mut current, &mut words);
                backtick = true;
            }
            '#' if current.is_empty() => {
                flush_word(&mut current, &mut words);
            }
            '(' => {
                flush_word(&mut current, &mut words);
                parens += 1;
            }
            ')' => {
                flush_word(&mut current, &mut words);
                parens -= 1;
            }
            '{' => {
                flush_word(&mut current, &mut words);
                braces += 1;
            }
            '}' => {
                flush_word(&mut current, &mut words);
                braces -= 1;
            }
            ch if ch.is_whitespace() || matches!(ch, ';' | '|' | '&') => {
                flush_word(&mut current, &mut words);
            }
            _ => current.push(ch),
        }
    }

    flush_word(&mut current, &mut words);

    if single_quote || double_quote || backtick || parens > 0 || braces > 0 {
        return true;
    }

    let mut blocks = Vec::new();

    for word in words {
        match word.as_str() {
            "if" => blocks.push("fi"),
            "for" | "while" | "until" | "select" => blocks.push("done"),
            "case" => blocks.push("esac"),
            "fi" | "done" | "esac" => {
                if blocks.last().copied() == Some(word.as_str()) {
                    blocks.pop();
                }
            }
            _ => {}
        }
    }

    !blocks.is_empty()
}

fn flush_word(current: &mut String, words: &mut Vec<String>) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
    }
}

#[cfg(test)]
mod tests {
    use super::needs_continuation;

    #[test]
    fn detects_multiline_shell_constructs() {
        assert!(needs_continuation("for host in 1 2; do"));
        assert!(!needs_continuation("for host in 1 2; do\n echo $host\ndone"));
        assert!(needs_continuation("if true; then"));
        assert!(!needs_continuation("if true; then\n echo ok\nfi"));
        assert!(needs_continuation("scan() {"));
        assert!(!needs_continuation("scan() {\n echo ok\n}"));
        assert!(needs_continuation("echo hello |"));
        assert!(needs_continuation("echo \"hello"));
        assert!(!needs_continuation("echo \"hello\""));
    }
}
