use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

use crate::core::{CommandContext, CommandOutput, ports::TextEditor};

use super::BuiltinCommand;

pub struct ConfigBuiltin {
    config_file: PathBuf,
    terminal_config_file: PathBuf,
    editor: Arc<dyn TextEditor>,
}

impl ConfigBuiltin {
    pub fn new(
        config_file: PathBuf,
        terminal_config_file: PathBuf,
        editor: Arc<dyn TextEditor>,
    ) -> Self {
        Self {
            config_file,
            terminal_config_file,
            editor,
        }
    }
}

impl BuiltinCommand for ConfigBuiltin {
    fn name(&self) -> &'static str {
        "sst-config"
    }

    fn help(&self) -> &'static str {
        "sst-config path|edit|terminal — configuración portable de SST"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("path") {
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.config_file.display()))),
            "edit" => {
                let args = vec![self.config_file.to_string_lossy().into_owned()];
                let status = self.editor.edit(&args, context.cwd)?;
                Ok(CommandOutput { status, stdout: String::new(), stderr: String::new() })
            }
            "terminal" => match args.get(1).map(String::as_str).unwrap_or("path") {
                "path" => Ok(CommandOutput::ok(format!(
                    "{}\n",
                    self.terminal_config_file.display()
                ))),
                "edit" => {
                    let edit_args =
                        vec![self.terminal_config_file.to_string_lossy().into_owned()];
                    let status = self.editor.edit(&edit_args, context.cwd)?;
                    Ok(CommandOutput {
                        status,
                        stdout: String::new(),
                        stderr: String::new(),
                    })
                }
                other => Ok(CommandOutput::error(
                    format!("sst-config terminal: subcomando desconocido: {other}"),
                    2,
                )),
            },
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


pub struct PathBuiltin;

impl BuiltinCommand for PathBuiltin {
    fn name(&self) -> &'static str {
        "sst-path"
    }

    fn help(&self) -> &'static str {
        "sst-path PATH — traduce rutas estilo /c/... a rutas Windows"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        let Some(raw) = args.first() else {
            return Ok(CommandOutput::error("sst-path: falta ruta", 2));
        };

        let path = crate::support::path::resolve(context.cwd, raw);
        Ok(CommandOutput::ok(format!("{}\n", path.display())))
    }
}
