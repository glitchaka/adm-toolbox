use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

use crate::core::{CommandContext, CommandOutput, ports::TextEditor};

use super::BuiltinCommand;

pub struct ConfigBuiltin {
    config_file: PathBuf,
    editor: Arc<dyn TextEditor>,
}

impl ConfigBuiltin {
    pub fn new(config_file: PathBuf, editor: Arc<dyn TextEditor>) -> Self {
        Self {
            config_file,
            editor,
        }
    }
}

impl BuiltinCommand for ConfigBuiltin {
    fn name(&self) -> &'static str {
        "adm-config"
    }

    fn help(&self) -> &'static str {
        "adm-config path|edit — configuración portable de la shell"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("path") {
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.config_file.display()))),
            "edit" => {
                self.editor.edit(&self.config_file)?;
                Ok(CommandOutput::ok(""))
            }
            "reload" => Ok(CommandOutput::error(
                "config reload debe ejecutarse mediante la función Bash 'config'",
                2,
            )),
            other => Ok(CommandOutput::error(
                format!("config: subcomando desconocido: {other}"),
                2,
            )),
        }
    }
}
