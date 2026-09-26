mod adapters;
mod app;
mod application;
mod builtins;
mod composition;
mod core;
mod presentation;
mod support;

use anyhow::Result;
use app::AdmToolbox;

fn main() -> Result<()> {
    AdmToolbox::new()?.run()
}
