#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellExecution {
    pub exit_requested: bool,
    pub status: i32,
}

impl ShellExecution {
    pub const fn continue_running(status: i32) -> Self {
        Self {
            exit_requested: false,
            status,
        }
    }

    pub const fn exit(status: i32) -> Self {
        Self {
            exit_requested: true,
            status,
        }
    }
}
