#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecution {
    pub exit_requested: bool,
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ShellExecution {
    pub const fn continue_running(status: i32) -> Self {
        Self {
            exit_requested: false,
            status,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub const fn exit(status: i32) -> Self {
        Self {
            exit_requested: true,
            status,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}
