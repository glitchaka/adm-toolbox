#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellExecution {
    pub exit_requested: bool,
}

impl ShellExecution {
    pub const fn continue_running() -> Self {
        Self {
            exit_requested: false,
        }
    }

    pub const fn exit() -> Self {
        Self {
            exit_requested: true,
        }
    }
}
