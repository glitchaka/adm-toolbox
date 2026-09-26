mod adapters;
mod app;
mod application;
mod builtins;
mod composition;
mod core;
mod presentation;
mod support;

use std::{env, path::Path};

use anyhow::Result;
use app::AdmToolbox;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut app = AdmToolbox::new()?;

    let status = if args.first().map(String::as_str) == Some("-c") {
        if args.len() < 2 {
            eprintln!("adm-toolbox: -c requiere un comando");
            2
        } else {
            app.run_command(&args[1..].join(" "))?
        }
    } else if let Some(script) = args.first() {
        app.run_script(Path::new(script))?
    } else {
        app.run()?;
        0
    };

    if status != 0 {
        std::process::exit(status);
    }

    Ok(())
}
