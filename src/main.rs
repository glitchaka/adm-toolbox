#![cfg_attr(windows, windows_subsystem = "windows")]

mod adapters;
mod app;
mod application;
mod builtins;
mod composition;
mod core;
mod presentation;
mod support;

use std::{env, io::{Read, Write}, path::{Path, PathBuf}};

use anyhow::Result;
use app::AdmToolbox;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    #[cfg(windows)]
    if args.is_empty() || args.first().is_some_and(|arg| matches!(arg.as_str(), "--gui" | "--console")) {
        if let Err(error) = presentation::gui::run() {
            let message = format!("Shell Shock Tool no pudo iniciarse: {error}");
            let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(), text.as_ptr(), text.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR); }
            std::process::exit(1);
        }
        return;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, System::Console::*};
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE { AttachConsole(ATTACH_PARENT_PROCESS); }
    }
    let status = match run_cli(&args) {
        Ok(status) => status,
        Err(error) => { eprintln!("adm-toolbox: {error}"); 2 }
    };
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(status);
}

#[derive(Default)]
struct BashInvocation {
    interactive: bool,
    login: bool,
    stdin_script: bool,
    no_rc: bool,
    no_profile: bool,
    command: Option<String>,
    shell_options: Vec<(String, bool)>,
    shopt_options: Vec<(String, bool)>,
    operands: Vec<String>,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_invocation(args: &[String]) -> Result<BashInvocation> {
    let mut invocation = BashInvocation::default();
    let mut index = 0usize;
    let mut parsing_options = true;

    while index < args.len() {
        let arg = &args[index];
        if !parsing_options {
            invocation.operands.extend(args[index..].iter().cloned());
            break;
        }

        match arg.as_str() {
            "--" => {
                parsing_options = false;
                index += 1;
                continue;
            }
            "--help" => {
                println!(
                    "Shell Shock Tool {} (Bash 5.3 compatible)\n\n\
                     -c COMANDO       ejecuta COMANDO\n\
                     -i               shell interactiva\n\
                     -l, --login      shell de login\n\
                     -s               lee comandos desde stdin\n\
                     -o OPCIÓN        activa opción de set\n\
                     +o OPCIÓN        desactiva opción de set\n\
                     -O OPCIÓN        activa shopt\n\
                     +O OPCIÓN        desactiva shopt\n\
                     --posix          activa modo POSIX\n\
                     --norc           no carga ~/.bashrc\n\
                     --noprofile      no carga archivos de perfil\n\
                     ARCHIVO [ARGS]   ejecuta script",
                    env!("CARGO_PKG_VERSION"),
                );
                std::process::exit(0);
            }
            "--version" => {
                println!("Shell Shock Tool {} — GNU Bash 5.3 compatibility layer", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--login" => invocation.login = true,
            "--norc" => invocation.no_rc = true,
            "--noprofile" => invocation.no_profile = true,
            "--posix" => invocation.shell_options.push(("posix".to_owned(), true)),
            "--restricted" => invocation.shopt_options.push(("restricted_shell".to_owned(), true)),
            "--verbose" => invocation.shell_options.push(("verbose".to_owned(), true)),
            "-c" => {
                index += 1;
                invocation.command = Some(args.get(index)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("-c requiere un comando"))?);
                index += 1;
                invocation.operands.extend(args[index..].iter().cloned());
                break;
            }
            "-s" => invocation.stdin_script = true,
            "-i" => invocation.interactive = true,
            "-l" => invocation.login = true,
            "-O" | "+O" => {
                let enabled = arg == "-O";
                index += 1;
                let option = args.get(index)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("{arg} requiere una opción shopt"))?;
                invocation.shopt_options.push((option, enabled));
            }
            "-o" | "+o" => {
                let enabled = arg == "-o";
                index += 1;
                let option = args.get(index)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("{arg} requiere una opción"))?;
                invocation.shell_options.push((option, enabled));
            }
            value if (value.starts_with('-') || value.starts_with('+')) && value.len() > 1 => {
                let enabled = value.starts_with('-');
                for flag in value[1..].chars() {
                    match flag {
                        'i' if enabled => invocation.interactive = true,
                        'l' if enabled => invocation.login = true,
                        's' if enabled => invocation.stdin_script = true,
                        'a' => invocation.shell_options.push(("allexport".to_owned(), enabled)),
                        'e' => invocation.shell_options.push(("errexit".to_owned(), enabled)),
                        'f' => invocation.shell_options.push(("noglob".to_owned(), enabled)),
                        'h' => invocation.shell_options.push(("hashall".to_owned(), enabled)),
                        'm' => invocation.shell_options.push(("monitor".to_owned(), enabled)),
                        'n' => invocation.shell_options.push(("noexec".to_owned(), enabled)),
                        'u' => invocation.shell_options.push(("nounset".to_owned(), enabled)),
                        'v' => invocation.shell_options.push(("verbose".to_owned(), enabled)),
                        'x' => invocation.shell_options.push(("xtrace".to_owned(), enabled)),
                        'B' => invocation.shell_options.push(("braceexpand".to_owned(), enabled)),
                        'C' => invocation.shell_options.push(("noclobber".to_owned(), enabled)),
                        'E' => invocation.shell_options.push(("errtrace".to_owned(), enabled)),
                        'H' => invocation.shell_options.push(("histexpand".to_owned(), enabled)),
                        'P' => invocation.shell_options.push(("physical".to_owned(), enabled)),
                        'T' => invocation.shell_options.push(("functrace".to_owned(), enabled)),
                        other => anyhow::bail!("opción desconocida: {}{}", if enabled { "-" } else { "+" }, other),
                    }
                }
            }
            _ => {
                parsing_options = false;
                invocation.operands.extend(args[index..].iter().cloned());
                break;
            }
        }
        index += 1;
    }

    Ok(invocation)
}

fn apply_invocation_options(
    engine: &mut dyn crate::core::ports::ShellEngine,
    invocation: &BashInvocation,
) -> Result<()> {
    for (option, enabled) in &invocation.shell_options {
        let command = format!("set {}o {}", if *enabled { "-" } else { "+" }, shell_quote(option));
        let result = engine.execute(&command)?;
        if result.status != 0 {
            anyhow::bail!("{}", result.stderr.trim());
        }
    }
    for (option, enabled) in &invocation.shopt_options {
        let command = format!("shopt {} {}", if *enabled { "-s" } else { "-u" }, shell_quote(option));
        let result = engine.execute(&command)?;
        if result.status != 0 {
            anyhow::bail!("{}", result.stderr.trim());
        }
    }
    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

fn source_startup_file(
    engine: &mut dyn crate::core::ports::ShellEngine,
    path: &Path,
) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let command = format!("source {}", shell_quote(&path.to_string_lossy()));
    let result = engine.execute(&command)?;
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    Ok(())
}

fn load_bash_startup(
    engine: &mut dyn crate::core::ports::ShellEngine,
    invocation: &BashInvocation,
) -> Result<()> {
    let Some(home) = home_dir() else { return Ok(()); };

    if invocation.login && !invocation.no_profile {
        for path in [
            home.join(".bash_profile"),
            home.join(".bash_login"),
            home.join(".profile"),
        ] {
            if path.is_file() {
                source_startup_file(engine, &path)?;
                break;
            }
        }
    }

    if invocation.interactive && !invocation.login && !invocation.no_rc {
        source_startup_file(engine, &home.join(".bashrc"))?;
    }

    if !invocation.interactive {
        if let Ok(path) = env::var("BASH_ENV") {
            if !path.is_empty() {
                source_startup_file(engine, &PathBuf::from(path))?;
            }
        }
    }

    Ok(())
}

fn run_cli(args: &[String]) -> Result<i32> {
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("--verify-terminal") {
        presentation::gui::verify_transport()?;
        return Ok(0);
    }

    if args.is_empty() {
        AdmToolbox::new()?.run()?;
        return Ok(0);
    }

    let invocation = parse_invocation(args)?;
    let (mut engine, command_names, paths) = composition::build_engine()?;
    engine.set_interactive(invocation.interactive);
    apply_invocation_options(engine.as_mut(), &invocation)?;
    load_bash_startup(engine.as_mut(), &invocation)?;

    if invocation.interactive && invocation.command.is_none() && invocation.operands.is_empty() && !invocation.stdin_script {
        let mut shell = presentation::shell::ShellSession::new(engine, command_names, paths.history_file())?;
        shell.run()?;
        return Ok(0);
    }

    let (source, script_name, positional) = if let Some(command) = invocation.command {
        let script_name = invocation.operands.first()
            .cloned()
            .unwrap_or_else(|| "bash".to_owned());
        let positional = invocation.operands.get(1..).unwrap_or(&[]).to_vec();
        (command, script_name, positional)
    } else if invocation.stdin_script || invocation.operands.is_empty() {
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source)?;
        let script_name = invocation.operands.first()
            .cloned()
            .unwrap_or_else(|| "bash".to_owned());
        let positional = invocation.operands.get(1..).unwrap_or(&[]).to_vec();
        (source, script_name, positional)
    } else {
        let script = &invocation.operands[0];
        let source = std::fs::read_to_string(script)?;
        (source, script.clone(), invocation.operands[1..].to_vec())
    };

    engine.set_arguments(&script_name, &positional);
    let result = engine.execute(&source)?;
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    Ok(result.status)
}
