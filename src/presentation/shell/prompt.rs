use std::{env, path::Path};

use crate::support::path;

pub fn render(cwd: &Path) -> String {
    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
    let cwd = path::display(cwd);

    format!(
        "\x1b[38;5;42m{user}@{host}\x1b[0m \x1b[38;5;39m{cwd}\x1b[0m\n$ "
    )
}

pub fn banner() -> &'static str {
    "\x1b[38;5;42mADM Toolbox 0.3.0\x1b[0m\nBash-compatible Rust administration shell\n"
}
