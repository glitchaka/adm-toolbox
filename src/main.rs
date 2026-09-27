#![cfg_attr(windows, windows_subsystem = "windows")]

mod adapters;
mod app;
mod application;
mod builtins;
mod composition;
mod core;
mod presentation;
mod support;

use std::{env, io::Write};

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

fn run_cli(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("Shell Shock Tool {} (adm-tool)\n\nSin argumentos: terminal propia\n--console: terminal propia\n-c COMANDO: ejecutar un comando\nARCHIVO.sh [ARGS...]: ejecutar un script\n--version: versión\n\nTerminal: selecciona con el mouse y usa Ctrl+C para copiar; Ctrl+V, Ctrl+Shift+V o Shift+Insert para pegar; la rueda recorre el historial visible.", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Some("--version") => { println!("Shell Shock Tool {} (adm-tool)", env!("CARGO_PKG_VERSION")); return Ok(0); }
        None => { AdmToolbox::new()?.run()?; return Ok(0); }
        #[cfg(windows)]
        Some("--verify-terminal") => { presentation::gui::verify_transport()?; return Ok(0); }
        _ => {}
    }
    let (mut engine, _, _) = composition::build_engine()?;
    let source = if args[0] == "-c" {
        let command = args.get(1).ok_or_else(|| anyhow::anyhow!("-c requiere un comando"))?;
        engine.set_arguments(args.get(2).map(String::as_str).unwrap_or("adm-toolbox"), args.get(3..).unwrap_or(&[]));
        command.clone()
    } else {
        if args[0].starts_with('-') { anyhow::bail!("opción desconocida: {}", args[0]); }
        engine.set_arguments(&args[0], &args[1..]);
        std::fs::read_to_string(&args[0])?
    };
    let result = engine.execute(&source)?;
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    Ok(result.status)
}
