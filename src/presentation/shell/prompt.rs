use std::{env, path::Path};

use crate::support::path;

pub fn render(cwd: &Path) -> String {
    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
    let cwd = path::display(cwd);

    format!(
        "\x1b[38;5;203m╭─ \u{f007} {user}@{host}\x1b[0m  \x1b[38;5;222m\u{f07c} {cwd}\x1b[0m\n\x1b[38;5;117m╰─ \u{f120} ❯\x1b[0m "
    )
}

pub fn banner() -> &'static str {
    "\x1b[1;38;5;117m\u{f120} Shell Shock Tool\x1b[0m  \x1b[38;5;203m( adm-tool )\x1b[0m\n\x1b[38;5;250mAdministración y diagnóstico en terreno · terminal portable\x1b[0m\nEscribe help para ver los comandos.\n"
}
