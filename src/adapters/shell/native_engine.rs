use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use anyhow::{Context, Result};

use crate::{
    builtins::CommandRegistry,
    core::{
        CommandContext,
        ShellExecution,
        bash::{ExecutionResult, Interpreter, ShellCommandHost},
        ports::ShellEngine,
    },
};

struct WindowsShellHost {
    registry: Arc<CommandRegistry>,
    interrupt: Arc<std::sync::atomic::AtomicBool>,
}

impl ShellCommandHost for WindowsShellHost {
    fn interrupted(&self) -> bool { self.interrupt.load(std::sync::atomic::Ordering::SeqCst) }
    fn execute_builtin(
        &self,
        name: &str,
        args: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>> {
        if name == "help" || name == "man" {
            let output = self.registry.help(args.first().map(String::as_str));
            return Ok(Some(ExecutionResult::from_parts(
                output.stdout,
                output.stderr,
                output.status,
            )));
        }

        if !self.registry.names().iter().any(|candidate| candidate == name) {
            return Ok(None);
        }

        let output = self.registry.execute(name, args, CommandContext { cwd, stdin })?;
        Ok(Some(ExecutionResult::from_parts(
            output.stdout,
            output.stderr,
            output.status,
        )))
    }

    fn execute_external(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .envs(env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if stdin.is_some() {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::inherit());
        }

        let mut child = command.spawn()
            .with_context(|| format!("no se pudo ejecutar {program}"))?;

        if let (Some(bytes), Some(mut writer)) = (stdin, child.stdin.take()) {
            writer.write_all(bytes)?;
        }

        let output = child.wait_with_output()?;
        Ok(ExecutionResult::from_parts(
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            output.status.code().unwrap_or(1),
        ))
    }
}

pub struct NativeShellEngine {
    interpreter: Interpreter,
    config_file: PathBuf,
    interrupt: Arc<std::sync::atomic::AtomicBool>,
}

impl NativeShellEngine {
    pub fn new(registry: Arc<CommandRegistry>, config_file: PathBuf) -> Result<Self> {
        let interrupt = crate::adapters::terminal::io::interrupt_flag();
        let host = WindowsShellHost { registry, interrupt: interrupt.clone() };
        let mut interpreter = Interpreter::new(Box::new(host));
        interpreter.env.export(
            "ADM_CONFIG",
            config_file.to_string_lossy().into_owned(),
        );

        let mut engine = Self {
            interpreter,
            config_file,
            interrupt,
        };
        engine.load_config()?;
        Ok(engine)
    }

    fn load_config(&mut self) -> Result<()> {
        if self.config_file.exists() {
            let source = std::fs::read_to_string(&self.config_file)?;
            let result = self.interpreter.execute_text(&source)?;
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
        }
        Ok(())
    }
}

impl ShellEngine for NativeShellEngine {
    fn set_arguments(&mut self, name: &str, args: &[String]) {
        self.interpreter.env.script_name = name.to_owned();
        self.interpreter.env.positional = args.to_vec();
    }
    fn working_dir(&self) -> &Path {
        &self.interpreter.env.cwd
    }

    fn execute(&mut self, line: &str) -> Result<ShellExecution> {
        self.interrupt.store(false, std::sync::atomic::Ordering::SeqCst);
        let result = self.interpreter.execute_text(line)?;

        let mut execution = if result.exit_requested {
            ShellExecution::exit(result.status)
        } else {
            ShellExecution::continue_running(result.status)
        };
        execution.stdout = result.stdout;
        execution.stderr = result.stderr;
        Ok(execution)
    }
}
