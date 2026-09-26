use std::{
    io::{Read, Write},
    sync::{Arc, OnceLock},
};

use anyhow::Result;
use brush_core::{
    ExecutionResult,
    builtins::{ContentOptions, ContentType, SimpleCommand},
    commands::ExecutionContext,
    extensions::ShellExtensions,
};

use crate::{
    builtins::CommandRegistry,
    core::CommandContext,
};

static REGISTRY: OnceLock<Arc<CommandRegistry>> = OnceLock::new();

pub fn install_registry(registry: Arc<CommandRegistry>) -> Result<()> {
    REGISTRY
        .set(registry)
        .map_err(|_| anyhow::anyhow!("el registro de comandos ya fue inicializado"))
}

pub fn registry() -> Result<&'static Arc<CommandRegistry>> {
    REGISTRY
        .get()
        .ok_or_else(|| anyhow::anyhow!("el registro de comandos no está inicializado"))
}

pub struct BrushBuiltinBridge;

impl SimpleCommand for BrushBuiltinBridge {
    fn get_content(
        name: &str,
        content_type: ContentType,
        _options: &ContentOptions,
    ) -> Result<String, brush_core::Error> {
        let content = match content_type {
            ContentType::ShortDescription => format!("{name} - ADM Toolbox builtin\n"),
            ContentType::ShortUsage => format!("{name}: {name} [ARGS...]\n"),
            ContentType::DetailedHelp | ContentType::ManPage => registry()
                .ok()
                .map(|registry| {
                    if name == "help" || name == "man" {
                        registry.help(None).stdout
                    } else {
                        registry.help(Some(name)).stdout
                    }
                })
                .unwrap_or_else(|| format!("{name} - ADM Toolbox builtin\n")),
        };

        Ok(content)
    }

    fn execute<SE: ShellExtensions, I: Iterator<Item = S>, S: AsRef<str>>(
        context: ExecutionContext<'_, SE>,
        args: I,
    ) -> Result<ExecutionResult, brush_core::Error> {
        let command_name = context.command_name.clone();
        let argv: Vec<String> = args
            .skip(1)
            .map(|value| value.as_ref().to_owned())
            .collect();

        let registry = match registry() {
            Ok(registry) => registry,
            Err(error) => {
                let _ = writeln!(context.stderr(), "adm: {error}");
                return Ok(ExecutionResult::general_error());
            }
        };

        let stdin_is_terminal = context.try_fd(0).is_none_or(|file| file.is_terminal());
        let mut input = Vec::new();

        let input_ref = if stdin_is_terminal {
            None
        } else {
            let mut stdin = context.stdin();
            match stdin.read_to_end(&mut input) {
                Ok(_) => Some(input.as_slice()),
                Err(error) => {
                    let _ = writeln!(context.stderr(), "{command_name}: {error}");
                    return Ok(ExecutionResult::general_error());
                }
            }
        };

        let command_context = CommandContext {
            cwd: context.shell.working_dir(),
            stdin: input_ref,
        };

        let output = if command_name == "help" || command_name == "man" {
            registry.help(argv.first().map(String::as_str))
        } else {
            match registry.execute(&command_name, &argv, command_context) {
                Ok(output) => output,
                Err(error) => {
                    let _ = writeln!(context.stderr(), "{command_name}: {error}");
                    return Ok(ExecutionResult::general_error());
                }
            }
        };

        if !output.stdout.is_empty() {
            let mut stdout = context.stdout();
            if stdout.write_all(output.stdout.as_bytes()).is_err() {
                return Ok(ExecutionResult::general_error());
            }
            let _ = stdout.flush();
        }

        if !output.stderr.is_empty() {
            let mut stderr = context.stderr();
            let _ = stderr.write_all(output.stderr.as_bytes());
            if !output.stderr.ends_with('\n') {
                let _ = writeln!(stderr);
            }
            let _ = stderr.flush();
        }

        Ok(ExecutionResult::new(status_code(output.status)))
    }
}

fn status_code(status: i32) -> u8 {
    status.clamp(0, u8::MAX as i32) as u8
}
