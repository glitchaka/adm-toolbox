use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use anyhow::Result;

use crate::core::{CommandContext, CommandOutput};

use super::BuiltinCommand;

pub struct CommandRegistry {
    commands: HashMap<String, Arc<dyn BuiltinCommand>>,
    primary_names: BTreeSet<String>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            primary_names: BTreeSet::new(),
        }
    }

    pub fn register(&mut self, command: Arc<dyn BuiltinCommand>) -> Result<()> {
        let primary = command.name().to_owned();
        self.insert_name(&primary, Arc::clone(&command))?;
        self.primary_names.insert(primary);

        for alias in command.aliases() {
            self.insert_name(alias, Arc::clone(&command))?;
        }

        Ok(())
    }

    pub fn names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }

    pub fn execute(
        &self,
        name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        let Some(command) = self.commands.get(name) else {
            return Ok(CommandOutput::error(
                format!("comando interno desconocido: {name}"),
                127,
            ));
        };

        command.execute(name, args, context)
    }

    pub fn help(&self, topic: Option<&str>) -> CommandOutput {
        if let Some(topic) = topic {
            let Some(command) = self.commands.get(topic) else {
                return CommandOutput::error(format!("help: tema desconocido: {topic}"), 1);
            };

            let mut help = command.help().to_owned();
            if !help.ends_with('\n') {
                help.push('\n');
            }
            return CommandOutput::ok(help);
        }

        let mut out = String::from(
            "ADM Toolbox\n\nShell Bash-compatible con comandos administrativos nativos.\n\nComandos ADM:\n",
        );

        for name in &self.primary_names {
            if let Some(command) = self.commands.get(name) {
                let first_line = command.help().lines().next().unwrap_or("");
                out.push_str(&format!("  {:<16} {}\n", name, first_line));
            }
        }

        out.push_str("\nUsa 'help COMANDO' o 'man COMANDO' para detalle.\n");
        CommandOutput::ok(out)
    }

    fn insert_name(&mut self, name: &str, command: Arc<dyn BuiltinCommand>) -> Result<()> {
        if self.commands.contains_key(name) {
            anyhow::bail!("builtin duplicado: {name}");
        }

        self.commands.insert(name.to_owned(), command);
        Ok(())
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
