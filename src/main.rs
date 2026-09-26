mod app;
mod commands;
mod editor;
mod shell;

use anyhow::Result;
use app::AdmToolbox;

fn main() -> Result<()> {
    AdmToolbox::new()?.run()
}
