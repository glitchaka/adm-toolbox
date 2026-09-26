use anyhow::Result;

use crate::core::{CommandContext, CommandOutput};

pub trait BuiltinCommand: Send + Sync {
    fn name(&self) -> &'static str;

    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    fn help(&self) -> &'static str;

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput>;
}
