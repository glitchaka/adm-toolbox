use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode};
use super::io;

use crate::core::ports::{TerminalFactory, TerminalKey, TerminalSession};

pub struct CrosstermTerminalFactory;

impl TerminalFactory for CrosstermTerminalFactory {
    fn alternate_screen(&self) -> Result<Box<dyn TerminalSession>> {
        io::enter()?;
        Ok(Box::new(CrosstermTerminalSession))
    }
}

struct CrosstermTerminalSession;

impl TerminalSession for CrosstermTerminalSession {
    fn size(&self) -> Result<(u16, u16)> {
        io::size()
    }

    fn clear(&mut self) -> Result<()> {
        io::write(b"\x1b[H\x1b[2J")?;
        Ok(())
    }

    fn write(&mut self, text: &str) -> Result<()> {
        io::write(text.as_bytes())?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn poll_key(&mut self, timeout: Duration) -> Result<Option<TerminalKey>> {
        if !io::poll(timeout)? {
            return Ok(None);
        }

        match io::read()? {
            Event::Key(key) => Ok(Some(map_key(key.code))),
            _ => Ok(None),
        }
    }

}

impl Drop for CrosstermTerminalSession {
    fn drop(&mut self) {
        io::leave();
    }
}

fn map_key(code: KeyCode) -> TerminalKey {
    match code {
        KeyCode::Char(' ') => TerminalKey::Space,
        KeyCode::Char(ch) => TerminalKey::Char(ch),
        KeyCode::Esc => TerminalKey::Escape,
        KeyCode::Up => TerminalKey::Up,
        KeyCode::Down => TerminalKey::Down,
        KeyCode::Left => TerminalKey::Left,
        KeyCode::Right => TerminalKey::Right,
        KeyCode::PageUp => TerminalKey::PageUp,
        KeyCode::PageDown => TerminalKey::PageDown,
        KeyCode::Home => TerminalKey::Home,
        KeyCode::End => TerminalKey::End,
        KeyCode::Enter => TerminalKey::Enter,
        KeyCode::Backspace => TerminalKey::Backspace,
        KeyCode::Delete => TerminalKey::Delete,
        _ => TerminalKey::Other,
    }
}
