use std::process::Command;

use anyhow::Result;

use crate::core::ports::{ProcessOutput, ProcessRunner};

pub struct WindowsProcessRunner;

impl ProcessRunner for WindowsProcessRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<ProcessOutput> {
        let output = Command::new(program).args(args).output()?;

        Ok(ProcessOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: output.status.code().unwrap_or(1),
        })
    }
}
