use std::sync::Arc;

use anyhow::Result;

use crate::{
    core::{CommandContext, CommandOutput, ports::TextEditor},
    support::path,
};

use super::BuiltinCommand;

pub struct EditorBuiltin {
    editor: Arc<dyn TextEditor>,
}

impl EditorBuiltin {
    pub fn new(editor: Arc<dyn TextEditor>) -> Self {
        Self { editor }
    }
}

impl BuiltinCommand for EditorBuiltin {
    fn name(&self) -> &'static str {
        "vim"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["edit"]
    }

    fn help(&self) -> &'static str {
        "vim FILE — editor modal integrado escrito en Rust"
    }

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        let Some(raw_path) = args.first() else {
            return Ok(CommandOutput::error(
                format!("{invoked_name}: falta el archivo"),
                2,
            ));
        };

        let file = path::resolve(context.cwd, raw_path);
        self.editor.edit(&file)?;
        Ok(CommandOutput::ok(""))
    }
}
