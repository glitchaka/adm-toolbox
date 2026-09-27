//! Terminal I/O supplied by the native window, with a console fallback for CLI use.
use std::{cell::RefCell, io::Write, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc}, time::Duration};
use anyhow::Result;
use crossterm::{event::Event, terminal};

pub struct WindowIo {
    pub output: mpsc::Sender<Vec<u8>>,
    pub input: mpsc::Receiver<Event>,
    pub size: Arc<Mutex<(u16, u16)>>,
    pub raw: Arc<AtomicBool>,
    pub interrupt: Arc<AtomicBool>,
    pub force_abort: Arc<AtomicBool>,
    pending: Option<Event>,
}
impl WindowIo {
    pub fn new(
        output: mpsc::Sender<Vec<u8>>,
        input: mpsc::Receiver<Event>,
        size: Arc<Mutex<(u16,u16)>>,
        raw: Arc<AtomicBool>,
        interrupt: Arc<AtomicBool>,
        force_abort: Arc<AtomicBool>,
    ) -> Self {
        Self { output, input, size, raw, interrupt, force_abort, pending: None }
    }
}
thread_local! { static WINDOW: RefCell<Option<WindowIo>> = const { RefCell::new(None) }; }
pub fn install(io: WindowIo) { WINDOW.with(|slot| *slot.borrow_mut() = Some(io)); }
pub fn interrupt_flag() -> Arc<AtomicBool> {
    WINDOW.with(|slot| slot.borrow().as_ref().map(|io| Arc::clone(&io.interrupt)))
        .unwrap_or_default()
}

pub fn force_abort_flag() -> Arc<AtomicBool> {
    WINDOW.with(|slot| slot.borrow().as_ref().map(|io| Arc::clone(&io.force_abort)))
        .unwrap_or_default()
}

pub fn output_sender() -> Option<mpsc::Sender<Vec<u8>>> {
    WINDOW.with(|slot| slot.borrow().as_ref().map(|io| io.output.clone()))
}

pub fn write(bytes: &[u8]) -> Result<()> {
    WINDOW.with(|slot| -> Result<()> {
        if let Some(io) = slot.borrow().as_ref() {
            let mut translated = Vec::with_capacity(bytes.len());
            for (index, &byte) in bytes.iter().enumerate() {
                if byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') { translated.push(b'\r'); }
                translated.push(byte);
            }
            io.output.send(translated)?;
        } else {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(bytes)?;
            stdout.flush()?;
        }
        Ok(())
    })
}
pub fn size() -> Result<(u16,u16)> {
    WINDOW.with(|slot| if let Some(io) = slot.borrow().as_ref() {
        Ok(*io.size.lock().unwrap_or_else(|e| e.into_inner()))
    } else { Ok(terminal::size()?) })
}
pub fn enter_raw() -> Result<()> {
    let embedded = WINDOW.with(|slot| if let Some(io) = slot.borrow().as_ref() {
        io.raw.store(true, Ordering::SeqCst);
        true
    } else {
        false
    });
    if !embedded {
        terminal::enable_raw_mode()?;
    }
    Ok(())
}

pub fn leave_raw() {
    let embedded = WINDOW.with(|slot| if let Some(io) = slot.borrow().as_ref() {
        io.raw.store(false, Ordering::SeqCst);
        true
    } else {
        false
    });
    if !embedded {
        let _ = terminal::disable_raw_mode();
    }
}

pub fn enter() -> Result<()> {
    enter_raw()?;
    write(b"\x1b[?1049h\x1b[?25l")
}

pub fn leave() {
    let _ = write(b"\x1b[?25h\x1b[?1049l");
    leave_raw();
}
pub fn poll(timeout: Duration) -> Result<bool> {
    WINDOW.with(|slot| -> Result<bool> {
        let mut slot = slot.borrow_mut();
        if let Some(io) = slot.as_mut() {
            if io.pending.is_some() { return Ok(true); }
            match io.input.recv_timeout(timeout) {
                Ok(event) => { io.pending = Some(event); Ok(true) }
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
                Err(error) => Err(error.into()),
            }
        } else { Ok(crossterm::event::poll(timeout)?) }
    })
}
pub fn read() -> Result<Event> {
    WINDOW.with(|slot| -> Result<Event> {
        let mut slot = slot.borrow_mut();
        if let Some(io) = slot.as_mut() {
            if let Some(event) = io.pending.take() { return Ok(event); }
            Ok(io.input.recv()?)
        } else { Ok(crossterm::event::read()?) }
    })
}
