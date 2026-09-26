use std::path::Path;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl CommandOutput {
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self { stdout: stdout.into(), stderr: String::new(), status: 0 }
    }

    pub fn error(stderr: impl Into<String>, status: i32) -> Self {
        Self { stdout: String::new(), stderr: stderr.into(), status }
    }
}

#[derive(Clone, Copy)]
pub struct CommandContext<'a> {
    pub cwd: &'a Path,
    pub stdin: Option<&'a [u8]>,
}
