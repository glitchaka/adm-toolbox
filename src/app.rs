use anyhow::Result;

use crate::shell::Shell;

pub struct AdmToolbox {
    shell: Shell,
}

impl AdmToolbox {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shell: Shell::new()?,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.shell.run()
    }
}
