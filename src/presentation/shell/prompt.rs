use std::{env, path::Path};

use crate::support::path;

pub fn render(cwd: &Path) -> String {
    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
    let cwd = path::display(cwd);

    format!(
        "\x1b[38;5;203m╭─ \u{f007} {user}@{host}\x1b[0m  \x1b[38;5;222m\u{f07c} {cwd}\x1b[0m\n\x1b[38;5;203m╰─ \u{f120} ❯\x1b[0m "
    )
}

pub fn banner() -> &'static str {
    "\x1b[38;5;203m\u{f120} ADM Toolbox\x1b[0m\nAdministración de Windows · terminal propia · Nerd Font\nEscribe help para ver los comandos.\n"
}
