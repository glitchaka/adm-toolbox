//! In-process interpreter session. The window talks to a Rust worker through channels.
use std::{fs, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc}, thread};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use super::io::{self, WindowIo};

enum WorkerRequest {
    Execute(String),
    Complete {
        line: String,
        cursor: usize,
        reply: mpsc::Sender<Vec<String>>,
    },
}

pub struct EmbeddedSession {
    commands: mpsc::Sender<WorkerRequest>, keys: mpsc::Sender<Event>, display: mpsc::Sender<Vec<u8>>,
    pub output: mpsc::Receiver<Vec<u8>>,
    size: Arc<Mutex<(u16,u16)>>, raw: Arc<AtomicBool>, busy: Arc<AtomicBool>,
    interrupt: Arc<AtomicBool>, force_abort: Arc<AtomicBool>, exited: Arc<AtomicBool>,
    line: Vec<char>, cursor: usize, history: Vec<String>, history_index: usize,
    pending: String,
}
impl EmbeddedSession {
    pub fn start(cols: u16, rows: u16) -> Result<Self> {
        let (commands, requests) = mpsc::channel::<WorkerRequest>();
        let (keys, key_events) = mpsc::channel();
        let (display, output) = mpsc::channel();
        let size = Arc::new(Mutex::new((cols, rows)));
        let raw = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(true));
        let interrupt = Arc::new(AtomicBool::new(false));
        let force_abort = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let terminal_io = WindowIo::new(
            display.clone(),
            key_events,
            size.clone(),
            raw.clone(),
            interrupt.clone(),
            force_abort.clone(),
        );
        let worker_busy = busy.clone(); let worker_exited = exited.clone();
        thread::spawn(move || {
            io::install(terminal_io);
            let result = (|| -> Result<()> {
                let (mut engine, _, paths) = crate::composition::build_engine()?;
                io::write(crate::presentation::shell::prompt::banner().as_bytes())?;
                io::write(crate::presentation::shell::prompt::render(engine.working_dir()).as_bytes())?;
                worker_busy.store(false, Ordering::SeqCst);
                for request in requests {
                    match request {
                        WorkerRequest::Execute(command) => {
                            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(paths.history_file()) {
                                use std::io::Write;
                                let _ = writeln!(file, "{}", command.replace('\n', " "));
                            }
                            match engine.execute(&command) {
                                Ok(result) => {
                                    io::write(result.stdout.as_bytes())?;
                                    io::write(result.stderr.as_bytes())?;
                                    if result.exit_requested { break; }
                                }
                                Err(error) => io::write(format!("adm: {error}\n").as_bytes())?,
                            }
                            io::write(crate::presentation::shell::prompt::render(engine.working_dir()).as_bytes())?;
                            worker_busy.store(false, Ordering::SeqCst);
                        }
                        WorkerRequest::Complete { line, cursor, reply } => {
                            let matches = engine.complete(&line, cursor).unwrap_or_default();
                            let _ = reply.send(matches);
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result { let _ = io::write(format!("\nError de terminal: {error}\n").as_bytes()); }
            worker_exited.store(true, Ordering::SeqCst);
        });
        let history = fs::read_to_string(crate::adapters::persistence::AppPaths::detect().history_file())
            .unwrap_or_default().lines().filter(|line| !line.starts_with('#')).map(str::to_owned).collect::<Vec<_>>();
        let history_index = history.len();
        Ok(Self { commands, keys, display, output, size, raw, busy, interrupt, force_abort, exited,
            line: Vec::new(), cursor: 0, history, history_index, pending: String::new() })
    }
    pub fn resize(&self, cols: u16, rows: u16) {
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = (cols, rows);
        if self.raw.load(Ordering::SeqCst) { let _ = self.keys.send(Event::Resize(cols, rows)); }
    }
    pub fn exited(&self) -> bool { self.exited.load(Ordering::SeqCst) }
    pub fn raw_mode(&self) -> bool { self.raw.load(Ordering::SeqCst) }
    pub fn send_raw_key(&self, key: KeyEvent) -> Result<()> {
        self.keys.send(Event::Key(key))?;
        Ok(())
    }

    pub fn force_abort(&self) {
        self.force_abort.store(true, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
    }
    fn emit(&self, text: &str) { let _ = self.display.send(text.as_bytes().to_vec()); }
    fn redraw(&self) {
        let prompt = if self.pending.is_empty() { "$ " } else { "> " };
        self.emit(&format!("\r\x1b[2K{prompt}{}", self.line.iter().collect::<String>()));
        let back = self.line.len() - self.cursor;
        if back > 0 { self.emit(&format!("\x1b[{back}D")); }
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(bytes);
        let special = match text.as_ref() {
            "\x1b[A" | "\x1bOA" => Some(KeyCode::Up), "\x1b[B" | "\x1bOB" => Some(KeyCode::Down),
            "\x1b[C" => Some(KeyCode::Right), "\x1b[D" => Some(KeyCode::Left),
            "\x1b[H" => Some(KeyCode::Home), "\x1b[F" => Some(KeyCode::End),
            "\x1b[3~" => Some(KeyCode::Delete), "\x1b[2~" => Some(KeyCode::Insert),
            "\x1b[5~" => Some(KeyCode::PageUp), "\x1b[6~" => Some(KeyCode::PageDown), _ => None,
        };
        let events = if let Some(code) = special { vec![KeyEvent::new(code, KeyModifiers::NONE)] }
            else { text.chars().map(|ch| match ch {
                '\r' | '\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\x7f' | '\x08' => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                '\x01'..='\x1a' => KeyEvent::new(KeyCode::Char((ch as u8 + b'a' - 1) as char), KeyModifiers::CONTROL),
                ch => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            }).collect() };
        for event in events {
            if self.raw.load(Ordering::SeqCst) { self.keys.send(Event::Key(event))?; continue; }
            if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
                self.interrupt.store(true, Ordering::SeqCst);
                self.line.clear(); self.cursor = 0; self.pending.clear();
                self.emit("^C\r\n");
                if !self.busy.load(Ordering::SeqCst) { self.redraw(); }
                continue;
            }
            if self.busy.load(Ordering::SeqCst) { continue; }
            match event.code {
                KeyCode::Enter => {
                    self.emit("\r\n");
                    self.pending.push_str(&self.line.iter().collect::<String>());
                    self.line.clear(); self.cursor = 0;
                    if crate::presentation::shell::session::needs_continuation(&self.pending) {
                        self.pending.push('\n'); self.redraw(); continue;
                    }
                    let command = std::mem::take(&mut self.pending);
                    if command.trim().is_empty() { self.redraw(); continue; }
                    self.history.push(command.clone()); self.history_index = self.history.len();
                    self.busy.store(true, Ordering::SeqCst);
                    self.commands.send(WorkerRequest::Execute(command))?;
                    continue;
                }
                KeyCode::Char('d') if event.modifiers.contains(KeyModifiers::CONTROL) && self.line.is_empty() => {
                    self.busy.store(true, Ordering::SeqCst); self.commands.send(WorkerRequest::Execute("exit".to_owned()))?;
                }
                KeyCode::Char('l') if event.modifiers.contains(KeyModifiers::CONTROL) => self.emit("\x1b[2J\x1b[H"),
                KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => { self.line.drain(..self.cursor); self.cursor = 0; }
                KeyCode::Char('a') if event.modifiers.contains(KeyModifiers::CONTROL) => self.cursor = 0,
                KeyCode::Char('e') if event.modifiers.contains(KeyModifiers::CONTROL) => self.cursor = self.line.len(),
                KeyCode::Char(ch) if !event.modifiers.contains(KeyModifiers::CONTROL) => { self.line.insert(self.cursor, ch); self.cursor += 1; }
                KeyCode::Backspace if self.cursor > 0 => { self.cursor -= 1; self.line.remove(self.cursor); }
                KeyCode::Delete if self.cursor < self.line.len() => { self.line.remove(self.cursor); }
                KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Right => self.cursor = (self.cursor + 1).min(self.line.len()),
                KeyCode::Home => self.cursor = 0, KeyCode::End => self.cursor = self.line.len(),
                KeyCode::Up if self.history_index > 0 => {
                    self.history_index -= 1; self.line = self.history[self.history_index].chars().collect(); self.cursor = self.line.len();
                }
                KeyCode::Down => {
                    self.history_index = (self.history_index + 1).min(self.history.len());
                    self.line = self.history.get(self.history_index).map(|s| s.chars().collect()).unwrap_or_default(); self.cursor = self.line.len();
                }
                KeyCode::Tab => self.complete(),
                _ => {},
            }
            self.redraw();
        }
        Ok(())
    }
    fn complete(&mut self) {
        let line = self.line.iter().collect::<String>();
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.commands.send(WorkerRequest::Complete {
            line: line.clone(),
            cursor: self.cursor,
            reply: reply_tx,
        }).is_err() {
            return;
        }

        let Ok(mut matches) = reply_rx.recv_timeout(std::time::Duration::from_secs(2)) else {
            return;
        };
        if matches.is_empty() {
            return;
        }

        matches.sort();
        matches.dedup();

        let prefix: String = self.line[..self.cursor].iter().collect();
        let start = prefix.rfind(|ch: char| {
            ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')')
        }).map_or(0, |index| index + 1);

        if matches.len() == 1 {
            let begin = prefix[..start].chars().count();
            let chars = matches[0].chars().collect::<Vec<_>>();
            self.line.splice(begin..self.cursor, chars.iter().copied());
            self.cursor = begin + chars.len();
        } else {
            self.emit(&format!("\r\n{}\r\n", matches.join("  ")));
        }
    }
}
