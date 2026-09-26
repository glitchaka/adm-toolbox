use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::system::SystemService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

const ALIASES: &[&str] = &[
    "ps", "top", "df", "free", "hostname", "whoami", "uname", "kill",
];

pub struct SystemBuiltin {
    service: Arc<SystemService>,
}

impl SystemBuiltin {
    pub fn new(service: Arc<SystemService>) -> Self {
        Self { service }
    }
}

impl BuiltinCommand for SystemBuiltin {
    fn name(&self) -> &'static str {
        "sys"
    }

    fn aliases(&self) -> &'static [&'static str] {
        ALIASES
    }

    fn help(&self) -> &'static str {
        "sys — información del equipo, procesos, memoria y discos"
    }

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        _context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if invoked_name == "sys" {
            self.service.execute(args)
        } else {
            self.service.execute_alias(invoked_name, args)
        }
    }
}
