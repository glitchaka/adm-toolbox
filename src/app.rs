use std::path::Path;

use anyhow::Result;

use crate::{composition, presentation::shell::ShellSession};

pub struct AdmToolbox {
    shell: ShellSession,
}

impl AdmToolbox {
    pub fn new() -> Result<Self> {
        Ok(Self {
            shell: composition::build_shell()?,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.shell.run()
    }

    pub fn run_command(&mut self, command: &str) -> Result<i32> {
        self.shell.execute_command(command)
    }

    pub fn run_script(&mut self, path: &Path) -> Result<i32> {
        let source = std::fs::read_to_string(path)?;
        self.shell.execute_command(&source)
    }
}
