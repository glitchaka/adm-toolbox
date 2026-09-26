pub mod guard;
mod crossterm_terminal;

pub use crossterm_terminal::CrosstermTerminalFactory;
#[cfg(windows)]
pub mod embedded;
pub mod io;
