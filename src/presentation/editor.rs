use std::{
    fs,
    io::{Write, stdout},
    path::Path,
};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};

use crate::{
    adapters::terminal::guard::AlternateScreenGuard,
    core::ports::TextEditor,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
    Command,
    Search,
    VisualLine,
}

struct VimEditor {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    offset: usize,
    mode: Mode,
    command: String,
    search: String,
    last_search: Option<String>,
    dirty: bool,
    pending_d: bool,
    pending_y: bool,
    pending_g: bool,
    register: Vec<Vec<char>>,
    undo: Vec<Vec<Vec<char>>>,
    redo: Vec<Vec<Vec<char>>>,
    visual_anchor: Option<usize>,
    line_numbers: bool,
    message: String,
}

pub struct ModalTextEditor;

impl TextEditor for ModalTextEditor {
    fn edit(&self, path: &Path) -> Result<()> {
        run_editor(path)
    }
}

fn run_editor(path: &Path) -> Result<()> {
    let _guard = AlternateScreenGuard::enter()?;
    let contents = fs::read_to_string(path).unwrap_or_default();

    let mut editor = VimEditor {
        lines: if contents.is_empty() {
            vec![Vec::new()]
        } else {
            contents.lines().map(|line| line.chars().collect()).collect()
        },
        row: 0,
        col: 0,
        offset: 0,
        mode: Mode::Normal,
        command: String::new(),
        search: String::new(),
        last_search: None,
        dirty: false,
        pending_d: false,
        pending_y: false,
        pending_g: false,
        register: Vec::new(),
        undo: Vec::new(),
        redo: Vec::new(),
        visual_anchor: None,
        line_numbers: true,
        message: String::new(),
    };

    loop {
        editor.render(path)?;

        if let Event::Key(key) = event::read()? {
            if editor.handle_key(key, path)? {
                break;
            }
        }
    }

    Ok(())
}

impl VimEditor {
    fn render(&mut self, path: &Path) -> Result<()> {
        let (width, height) = terminal::size()?;
        let body_height = height.saturating_sub(2) as usize;

        if self.row < self.offset {
            self.offset = self.row;
        } else if self.row >= self.offset + body_height.max(1) {
            self.offset = self.row.saturating_sub(body_height.saturating_sub(1));
        }

        let mut out = stdout();
        queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

        for screen_row in 0..body_height {
            let line_index = self.offset + screen_row;
            queue!(out, cursor::MoveTo(0, screen_row as u16))?;

            if let Some(line) = self.lines.get(line_index) {
                let selected = self.visual_line_contains(line_index);

                if selected {
                    queue!(out, SetBackgroundColor(Color::DarkGrey))?;
                }

                if self.line_numbers {
                    queue!(
                        out,
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!("{:>5} ", line_index + 1)),
                        ResetColor
                    )?;

                    if selected {
                        queue!(out, SetBackgroundColor(Color::DarkGrey))?;
                    }
                }

                let gutter = if self.line_numbers { 6 } else { 0 };
                let available = width.saturating_sub(gutter) as usize;
                let visible: String = line.iter().copied().take(available).collect();
                queue!(out, Print(visible), ResetColor)?;
            } else if self.line_numbers {
                queue!(
                    out,
                    SetForegroundColor(Color::DarkGrey),
                    Print("    ~ "),
                    ResetColor
                )?;
            } else {
                queue!(
                    out,
                    SetForegroundColor(Color::DarkGrey),
                    Print("~"),
                    ResetColor
                )?;
            }
        }

        let mode_text = match self.mode {
            Mode::Normal => " NORMAL ",
            Mode::Insert => " INSERT ",
            Mode::Command => " COMMAND ",
            Mode::Search => " SEARCH ",
            Mode::VisualLine => " VISUAL LINE ",
        };

        queue!(
            out,
            cursor::MoveTo(0, height.saturating_sub(2)),
            SetBackgroundColor(Color::DarkBlue),
            SetForegroundColor(Color::White),
            Clear(ClearType::CurrentLine),
            Print(mode_text),
            Print(if self.dirty { " [+] " } else { "     " }),
            Print(path.display().to_string()),
            ResetColor
        )?;

        queue!(
            out,
            cursor::MoveTo(0, height.saturating_sub(1)),
            Clear(ClearType::CurrentLine)
        )?;

        match self.mode {
            Mode::Command => queue!(out, Print(":"), Print(&self.command))?,
            Mode::Search => queue!(out, Print("/"), Print(&self.search))?,
            _ if !self.message.is_empty() => {
                queue!(
                    out,
                    SetForegroundColor(Color::Yellow),
                    Print(&self.message),
                    ResetColor
                )?
            }
            _ => queue!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print("Esc normal · i insertar · V visual línea · / buscar · :w guardar · :q salir"),
                ResetColor
            )?,
        }

        let (cursor_x, cursor_y) = match self.mode {
            Mode::Command => (
                1_u16.saturating_add(self.command.chars().count() as u16),
                height.saturating_sub(1),
            ),
            Mode::Search => (
                1_u16.saturating_add(self.search.chars().count() as u16),
                height.saturating_sub(1),
            ),
            _ => (
                (if self.line_numbers { 6_u16 } else { 0_u16 })
                    .saturating_add(self.col as u16),
                self.row.saturating_sub(self.offset) as u16,
            ),
        };

        if cursor_y < height && cursor_x < width {
            queue!(out, cursor::MoveTo(cursor_x, cursor_y), cursor::Show)?;
        }

        out.flush()?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, path: &Path) -> Result<bool> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.mode = Mode::Normal;
            self.command.clear();
            self.search.clear();
            self.visual_anchor = None;
            self.reset_pending();
            return Ok(false);
        }

        if !matches!(self.mode, Mode::Command | Mode::Search) {
            self.message.clear();
        }

        match self.mode {
            Mode::Normal => self.normal_key(key),
            Mode::Insert => self.insert_key(key),
            Mode::Command => return self.command_key(key, path),
            Mode::Search => self.search_key(key),
            Mode::VisualLine => self.visual_line_key(key),
        }

        Ok(false)
    }

    fn normal_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            self.redo();
            return;
        }

        match key.code {
            KeyCode::Esc => self.reset_pending(),
            KeyCode::Char('h') | KeyCode::Left => self.move_left(),
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('l') | KeyCode::Right => self.move_right(),
            KeyCode::Char('w') => self.next_word(),
            KeyCode::Char('b') => self.prev_word(),
            KeyCode::Char('i') => self.mode = Mode::Insert,
            KeyCode::Char('a') => {
                if self.col < self.current_len() {
                    self.col += 1;
                }
                self.mode = Mode::Insert;
            }
            KeyCode::Char('o') => {
                self.snapshot();
                self.row += 1;
                self.lines.insert(self.row, Vec::new());
                self.col = 0;
                self.mode = Mode::Insert;
                self.dirty = true;
            }
            KeyCode::Char('O') => {
                self.snapshot();
                self.lines.insert(self.row, Vec::new());
                self.col = 0;
                self.mode = Mode::Insert;
                self.dirty = true;
            }
            KeyCode::Char('x') => self.delete_char(),
            KeyCode::Char('d') => {
                if self.pending_d {
                    self.delete_line();
                    self.pending_d = false;
                } else {
                    self.pending_d = true;
                    self.pending_y = false;
                }
            }
            KeyCode::Char('y') => {
                if self.pending_y {
                    self.yank_line();
                    self.pending_y = false;
                } else {
                    self.pending_y = true;
                    self.pending_d = false;
                }
            }
            KeyCode::Char('p') => self.paste_after(),
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char('V') => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(self.row);
            },
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.row = 0;
                    self.col = 0;
                    self.pending_g = false;
                } else {
                    self.pending_g = true;
                }
            }
            KeyCode::Char('G') => {
                self.row = self.lines.len().saturating_sub(1);
                self.clamp_col();
            }
            KeyCode::Char('0') => self.col = 0,
            KeyCode::Char('$') => self.col = self.current_len().saturating_sub(1),
            KeyCode::Char(':') => {
                self.mode = Mode::Command;
                self.command.clear();
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.search.clear();
            }
            KeyCode::Char('n') => self.repeat_search(true),
            KeyCode::Char('N') => self.repeat_search(false),
            _ => self.reset_pending(),
        }
    }

    fn insert_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                if self.col > 0 && self.col == self.current_len() {
                    self.col -= 1;
                }
            }
            KeyCode::Enter => {
                self.snapshot();
                let split_at = self.col.min(self.current_len());
                let tail = self.lines[self.row].split_off(split_at);
                self.row += 1;
                self.lines.insert(self.row, tail);
                self.col = 0;
                self.dirty = true;
            }
            KeyCode::Backspace => {
                if self.col > 0 {
                    self.snapshot();
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                    self.dirty = true;
                } else if self.row > 0 {
                    self.snapshot();
                    let current = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].extend(current);
                    self.dirty = true;
                }
            }
            KeyCode::Delete => {
                if self.col < self.current_len() {
                    self.snapshot();
                    self.lines[self.row].remove(self.col);
                    self.dirty = true;
                }
            }
            KeyCode::Left => self.move_left(),
            KeyCode::Right => {
                if self.col < self.current_len() {
                    self.col += 1;
                }
            }
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Char(ch) => {
                self.snapshot();
                let col = self.col.min(self.current_len());
                self.lines[self.row].insert(col, ch);
                self.col = col + 1;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn command_key(&mut self, key: KeyEvent, path: &Path) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.command.clear();
            }
            KeyCode::Backspace => {
                self.command.pop();
            }
            KeyCode::Char(ch) => self.command.push(ch),
            KeyCode::Enter => {
                let command = self.command.trim().to_owned();
                self.command.clear();
                self.mode = Mode::Normal;

                match command.as_str() {
                    "w" => {
                        self.save(path)?;
                        self.message = format!("{} líneas escritas", self.lines.len());
                    }
                    "q" => {
                        if !self.dirty {
                            return Ok(true);
                        }
                        self.message =
                            "E37: hay cambios sin guardar; usa :q! o :wq".to_owned();
                    }
                    "q!" => return Ok(true),
                    "wq" | "x" => {
                        self.save(path)?;
                        return Ok(true);
                    }
                    "set number" | "set nu" => {
                        self.line_numbers = true;
                    }
                    "set nonumber" | "set nonu" => {
                        self.line_numbers = false;
                    }
                    _ if command.starts_with("%s/") => {
                        self.substitute_all(&command)?;
                    }
                    _ => {
                        self.message = format!("No es un comando del editor: {command}");
                    }
                }
            }
            _ => {}
        }

        Ok(false)
    }

    fn search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.search.clear();
            }
            KeyCode::Backspace => {
                self.search.pop();
            }
            KeyCode::Char(ch) => self.search.push(ch),
            KeyCode::Enter => {
                if !self.search.is_empty() {
                    self.last_search = Some(self.search.clone());
                    let pattern = self.search.clone();
                    self.find_next(&pattern, true);
                }
                self.search.clear();
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn visual_line_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('V') => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('g') => {
                self.row = 0;
                self.clamp_col();
            }
            KeyCode::Char('G') => {
                self.row = self.lines.len().saturating_sub(1);
                self.clamp_col();
            }
            KeyCode::Char('y') => {
                self.yank_visual_lines();
                self.mode = Mode::Normal;
                self.visual_anchor = None;
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                self.delete_visual_lines();
                self.mode = Mode::Normal;
                self.visual_anchor = None;
            }
            _ => {}
        }
    }

    fn visual_line_contains(&self, row: usize) -> bool {
        if self.mode != Mode::VisualLine {
            return false;
        }

        let Some(anchor) = self.visual_anchor else {
            return false;
        };

        let start = anchor.min(self.row);
        let end = anchor.max(self.row);
        (start..=end).contains(&row)
    }

    fn visual_line_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        Some((anchor.min(self.row), anchor.max(self.row)))
    }

    fn yank_visual_lines(&mut self) {
        let Some((start, end)) = self.visual_line_range() else {
            return;
        };

        self.register = self.lines[start..=end].to_vec();
        self.message = format!("{} línea(s) copiada(s)", end - start + 1);
    }

    fn delete_visual_lines(&mut self) {
        let Some((start, end)) = self.visual_line_range() else {
            return;
        };

        self.snapshot();
        self.register = self.lines[start..=end].to_vec();
        self.lines.drain(start..=end);

        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }

        self.row = start.min(self.lines.len().saturating_sub(1));
        self.col = 0;
        self.dirty = true;
        self.message = format!("{} línea(s) eliminada(s)", end - start + 1);
    }

    fn substitute_all(&mut self, command: &str) -> Result<()> {
        let body = command.trim_start_matches("%s/");
        let mut parts = body.split('/');

        let Some(pattern) = parts.next() else {
            return Ok(());
        };
        let Some(replacement) = parts.next() else {
            self.message = "uso: :%s/antiguo/nuevo/g".to_owned();
            return Ok(());
        };

        if pattern.is_empty() {
            self.message = "patrón vacío".to_owned();
            return Ok(());
        }

        let flags = parts.next().unwrap_or("");
        let replace_all = flags.contains('g');
        self.snapshot();

        let mut count = 0_usize;

        for line in &mut self.lines {
            let source: String = line.iter().copied().collect();
            let occurrences = source.matches(pattern).count();

            if occurrences == 0 {
                continue;
            }

            let replaced = if replace_all {
                count += occurrences;
                source.replace(pattern, replacement)
            } else {
                count += 1;
                source.replacen(pattern, replacement, 1)
            };

            *line = replaced.chars().collect();
        }

        if count > 0 {
            self.dirty = true;
            self.message = format!("{count} sustitución(es)");
            self.clamp_col();
        } else {
            self.message = format!("E486: patrón no encontrado: {pattern}");
        }

        Ok(())
    }

    fn save(&mut self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let text = self
            .lines
            .iter()
            .map(|line| line.iter().copied().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        fs::write(path, text)?;
        self.dirty = false;
        Ok(())
    }

    fn snapshot(&mut self) {
        self.undo.push(self.lines.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.lines.clone());
            self.lines = previous;
            self.row = self.row.min(self.lines.len().saturating_sub(1));
            self.clamp_col();
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.lines.clone());
            self.lines = next;
            self.row = self.row.min(self.lines.len().saturating_sub(1));
            self.clamp_col();
            self.dirty = true;
        }
    }

    fn delete_char(&mut self) {
        if self.col < self.current_len() {
            self.snapshot();
            self.lines[self.row].remove(self.col);
            self.dirty = true;
            self.clamp_col();
        }
    }

    fn delete_line(&mut self) {
        self.snapshot();
        self.register = vec![self.lines[self.row].clone()];

        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(self.row);
            self.row = self.row.min(self.lines.len() - 1);
        }

        self.col = 0;
        self.dirty = true;
    }

    fn yank_line(&mut self) {
        self.register = vec![self.lines[self.row].clone()];
    }

    fn paste_after(&mut self) {
        if self.register.is_empty() {
            return;
        }

        self.snapshot();
        let insert_at = (self.row + 1).min(self.lines.len());

        for (offset, line) in self.register.clone().into_iter().enumerate() {
            self.lines.insert(insert_at + offset, line);
        }

        self.row = insert_at;
        self.col = 0;
        self.dirty = true;
    }

    fn next_word(&mut self) {
        if self.current_len() == 0 {
            if self.row + 1 < self.lines.len() {
                self.row += 1;
                self.col = 0;
            }
            return;
        }

        let line = &self.lines[self.row];
        let mut index = self.col.saturating_add(1);

        while index < line.len() && line[index].is_alphanumeric() {
            index += 1;
        }

        while index < line.len() && !line[index].is_alphanumeric() {
            index += 1;
        }

        if index < line.len() {
            self.col = index;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn prev_word(&mut self) {
        if self.col == 0 {
            if self.row > 0 {
                self.row -= 1;
                self.col = self.current_len().saturating_sub(1);
            }
            return;
        }

        let line = &self.lines[self.row];
        let mut index = self.col.saturating_sub(1);

        while index > 0 && !line[index].is_alphanumeric() {
            index -= 1;
        }

        while index > 0 && line[index - 1].is_alphanumeric() {
            index -= 1;
        }

        self.col = index;
    }

    fn repeat_search(&mut self, forward: bool) {
        if let Some(pattern) = self.last_search.clone() {
            self.find_next(&pattern, forward);
        }
    }

    fn find_next(&mut self, pattern: &str, forward: bool) {
        if pattern.is_empty() || self.lines.is_empty() {
            return;
        }

        let total = self.lines.len();

        for step in 1..=total {
            let row = if forward {
                (self.row + step) % total
            } else {
                (self.row + total - (step % total)) % total
            };

            let text: String = self.lines[row].iter().copied().collect();

            if let Some(byte_pos) = text.find(pattern) {
                self.row = row;
                self.col = text[..byte_pos].chars().count();
                return;
            }
        }
    }

    fn move_left(&mut self) {
        self.col = self.col.saturating_sub(1);
    }

    fn move_right(&mut self) {
        let len = self.current_len();
        if len > 0 {
            self.col = (self.col + 1).min(len - 1);
        }
    }

    fn move_up(&mut self) {
        self.row = self.row.saturating_sub(1);
        self.clamp_col();
    }

    fn move_down(&mut self) {
        self.row = (self.row + 1).min(self.lines.len().saturating_sub(1));
        self.clamp_col();
    }

    fn current_len(&self) -> usize {
        self.lines.get(self.row).map(Vec::len).unwrap_or(0)
    }

    fn clamp_col(&mut self) {
        let len = self.current_len();
        self.col = if len == 0 { 0 } else { self.col.min(len - 1) };
    }

    fn reset_pending(&mut self) {
        self.pending_d = false;
        self.pending_y = false;
        self.pending_g = false;
    }
}
