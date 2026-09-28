use anyhow::Result;

use crate::{composition, presentation::shell::ShellSession};

pub struct ShellShockTool {
    shell: ShellSession,
}

impl ShellShockTool {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shell: composition::build_shell()?,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.shell.run()
    }

}
