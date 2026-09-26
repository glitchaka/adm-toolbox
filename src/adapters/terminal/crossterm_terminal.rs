use std::io::{Write, stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::core::ports::{TerminalFactory, TerminalKey, TerminalSession};

pub struct CrosstermTerminalFactory;

impl TerminalFactory for CrosstermTerminalFactory {
    fn alternate_screen(&self) -> Result<Box<dyn TerminalSession>> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(Box::new(CrosstermTerminalSession))
    }
}

struct CrosstermTerminalSession;

impl TerminalSession for CrosstermTerminalSession {
    fn size(&self) -> Result<(u16, u16)> {
        Ok(terminal::size()?)
    }

    fn clear(&mut self) -> Result<()> {
        execute!(stdout(), cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        Ok(())
    }

    fn write(&mut self, text: &str) -> Result<()> {
        stdout().write_all(text.as_bytes())?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        stdout().flush()?;
        Ok(())
    }

    fn poll_key(&mut self, timeout: Duration) -> Result<Option<TerminalKey>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }

        match event::read()? {
            Event::Key(key) => Ok(Some(map_key(key.code))),
            _ => Ok(None),
        }
    }

    fn read_key(&mut self) -> Result<TerminalKey> {
        loop {
            if let Event::Key(key) = event::read()? {
                return Ok(map_key(key.code));
            }
        }
    }
}

impl Drop for CrosstermTerminalSession {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
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
