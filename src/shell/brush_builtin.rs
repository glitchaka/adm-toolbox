use std::io::{Read, Write};

use brush_core::{
    ExecutionResult,
    builtins::{ContentOptions, ContentType, SimpleCommand},
    commands::ExecutionContext,
    extensions::ShellExtensions,
};

use crate::{commands, editor};

use super::{portable_config_path, resolve_path};

pub struct AdmBuiltin;

impl SimpleCommand for AdmBuiltin {
    fn get_content(
        name: &str,
        content_type: ContentType,
        _options: &ContentOptions,
    ) -> Result<String, brush_core::Error> {
        let text = match content_type {
            ContentType::ShortDescription => {
                format!("{name} - ADM Toolbox Rust command\n")
            }
            ContentType::ShortUsage => format!("{name}: {name} [ARGS...]\n"),
            ContentType::DetailedHelp | ContentType::ManPage => {
                builtin_help(name)
            }
        };

        Ok(text)
    }

    fn execute<SE: ShellExtensions, I: Iterator<Item = S>, S: AsRef<str>>(
        context: ExecutionContext<'_, SE>,
        args: I,
    ) -> Result<ExecutionResult, brush_core::Error> {
        let name = context.command_name.clone();
        let argv: Vec<String> = args
            .skip(1)
            .map(|value| value.as_ref().to_owned())
            .collect();

        if matches!(name.as_str(), "vim" | "edit") {
            return run_editor(context, &name, &argv);
        }

        if name == "adm-config" {
            return run_config(context, &argv);
        }

        if name == "adm-path" {
            return run_path_converter(context, &argv);
        }

        let cwd = context.shell.working_dir().to_path_buf();
        let stdin_is_terminal = context.try_fd(0).is_none_or(|file| file.is_terminal());
        let mut input = Vec::new();

        let input_ref = if stdin_is_terminal {
            None
        } else {
            let mut stdin = context.stdin();
            match stdin.read_to_end(&mut input) {
                Ok(_) => Some(input.as_slice()),
                Err(error) => {
                    let _ = writeln!(context.stderr(), "{name}: {error}");
                    return Ok(ExecutionResult::general_error());
                }
            }
        };

        match commands::run(&name, &argv, input_ref, &cwd) {
            Ok(output) => {
                if !output.stdout.is_empty() {
                    let mut stdout = context.stdout();
                    if let Err(error) = stdout.write_all(output.stdout.as_bytes()) {
                        let _ = writeln!(context.stderr(), "{name}: {error}");
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
            Err(error) => {
                let _ = writeln!(context.stderr(), "{name}: {error}");
                Ok(ExecutionResult::general_error())
            }
        }
    }
}

fn run_editor<SE: ShellExtensions>(
    context: ExecutionContext<'_, SE>,
    name: &str,
    args: &[String],
) -> Result<ExecutionResult, brush_core::Error> {
    let Some(raw_path) = args.first() else {
        let _ = writeln!(context.stderr(), "{name}: falta el archivo");
        return Ok(ExecutionResult::new(2));
    };

    let path = resolve_path(context.shell.working_dir(), raw_path);

    match editor::run(&path) {
        Ok(()) => Ok(ExecutionResult::success()),
        Err(error) => {
            let _ = writeln!(context.stderr(), "{name}: {error}");
            Ok(ExecutionResult::general_error())
        }
    }
}

fn run_config<SE: ShellExtensions>(
    context: ExecutionContext<'_, SE>,
    args: &[String],
) -> Result<ExecutionResult, brush_core::Error> {
    let path = portable_config_path();

    match args.first().map(String::as_str).unwrap_or("path") {
        "path" => {
            let _ = writeln!(context.stdout(), "{}", path.display());
            Ok(ExecutionResult::success())
        }
        "edit" => match editor::run(&path) {
            Ok(()) => Ok(ExecutionResult::success()),
            Err(error) => {
                let _ = writeln!(context.stderr(), "config: {error}");
                Ok(ExecutionResult::general_error())
            }
        },
        "reload" => {
            let _ = writeln!(
                context.stderr(),
                "config reload debe ejecutarse mediante la función de shell 'config'"
            );
            Ok(ExecutionResult::new(2))
        }
        other => {
            let _ = writeln!(
                context.stderr(),
                "config: subcomando desconocido: {other}; usa path, edit o reload"
            );
            Ok(ExecutionResult::new(2))
        }
    }
}

fn run_path_converter<SE: ShellExtensions>(
    context: ExecutionContext<'_, SE>,
    args: &[String],
) -> Result<ExecutionResult, brush_core::Error> {
    let Some(raw) = args.first() else {
        let _ = writeln!(context.stderr(), "adm-path: falta ruta");
        return Ok(ExecutionResult::new(2));
    };

    let path = resolve_path(context.shell.working_dir(), raw);
    let _ = writeln!(context.stdout(), "{}", path.display());
    Ok(ExecutionResult::success())
}

fn status_code(status: i32) -> u8 {
    status.clamp(0, u8::MAX as i32) as u8
}

pub fn builtin_names() -> &'static [&'static str] {
    &[
        "net",
        "sys",
        "domain",
        "device",
        "switch",
        "wol",
        "diag",
        "man",
        "env",
        "clear",
        "ls",
        "cat",
        "head",
        "tail",
        "grep",
        "wc",
        "sort",
        "uniq",
        "cut",
        "tee",
        "less",
        "more",
        "sed",
        "awk",
        "diff",
        "sha256sum",
        "base64",
        "find",
        "basename",
        "dirname",
        "realpath",
        "date",
        "sleep",
        "touch",
        "mkdir",
        "rm",
        "cp",
        "mv",
        "which",
        "ps",
        "top",
        "df",
        "free",
        "hostname",
        "whoami",
        "uname",
        "kill",
        "vim",
        "edit",
        "adm-config",
        "adm-path",
    ]
}

fn builtin_help(name: &str) -> String {
    match name {
        "net" => "net - red, descubrimiento y tráfico\nUsa: net --help\n".to_owned(),
        "sys" => "sys - información del equipo y procesos\nUsa: sys --help\n".to_owned(),
        "domain" => "domain - pertenencia a Active Directory/dominio\nUsa: domain --help\n".to_owned(),
        "device" => "device - inventario de equipos\nUsa: device --help\n".to_owned(),
        "switch" => "switch - resolución MAC → switch → puerto\nUsa: switch --help\n".to_owned(),
        "vim" | "edit" => "vim FILE - editor modal escrito en Rust\n".to_owned(),
        _ => format!("{name} - comando Rust integrado de ADM Toolbox\n"),
    }
}
