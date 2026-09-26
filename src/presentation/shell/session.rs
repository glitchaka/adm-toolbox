use std::{
    path::PathBuf,
};

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

        while self.running {
            let prompt = prompt::render(self.engine.working_dir());

            match self.editor.readline(&prompt) {
                Ok(line) => {
                    let line = line.trim_end();

                    if line.trim().is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(line);

                    match self.engine.execute(line) {
                        Ok(result) => {
                            if result.exit_requested {
                                self.running = false;
                            }
                        }
                        Err(error) => eprintln!("adm: {error}"),
                    }
                }
                Err(ReadlineError::Interrupted) => println!("^C"),
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
