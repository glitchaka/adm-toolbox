//! A native terminal window: our renderer, our interpreter, one executable.
use std::{ptr::{null, null_mut}, time::{Duration, Instant}};
use anyhow::{bail, Result};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*}};
use crate::adapters::terminal::embedded::EmbeddedSession;

const PAD: i32 = 14;
const BG: u32 = 0x181512;
const FG: u32 = 0xDEDAD5;

struct Terminal {
    pty: EmbeddedSession,
    parser: vt100::Parser,
    font: HFONT,
    bold: HFONT,
    cell_width: i32,
    cell_height: i32,
    selection: Option<(usize, usize)>,
    dragging: bool,
    suppress_char: bool,
    surrogate: Option<u16>,
    cursor_on: bool,
    blink: Instant,
    font_family: String,
}

fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

pub fn run() -> Result<()> {
    unsafe {
        SetProcessDPIAware();
        let instance = GetModuleHandleW(null());
        let class = wide("AdmToolboxTerminal");
        let wc = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance,
            lpszClassName: class.as_ptr(), hCursor: LoadCursorW(null_mut(), IDC_IBEAM),
            ..std::mem::zeroed() };
        if RegisterClassW(&wc) == 0 { bail!("No se pudo registrar la terminal: {}", std::io::Error::last_os_error()); }
        let pty = EmbeddedSession::start(100, 30)?;
        let (font, bold, font_family) = make_terminal_fonts(19);
        let state = Box::new(Terminal { pty, parser: vt100::Parser::new(30, 100, 10000), font, bold,
            cell_width: 10, cell_height: 23, selection: None, dragging: false,
            suppress_char: false, surrogate: None, cursor_on: true, blink: Instant::now(),
            font_family });
        let ptr = Box::into_raw(state);
        let window_title = if state.font_family.to_ascii_lowercase().contains("nerd font")
            || state.font_family.to_ascii_lowercase().ends_with(" nf")
        {
            format!("ADM Toolbox — {}", state.font_family)
        } else {
            format!("ADM Toolbox — {} (Nerd Font no instalada)", state.font_family)
        };
        let hwnd = CreateWindowExW(0, class.as_ptr(), wide(&window_title).as_ptr(),
            WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 1080, 760,
            null_mut(), null_mut(), instance, ptr.cast());
        if hwnd.is_null() { drop(Box::from_raw(ptr)); bail!("No se pudo crear la terminal: {}", std::io::Error::last_os_error()); }
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result == 0 { break; }
            if result == -1 { bail!("Error en el bucle de ventanas"); }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

const NERD_FONT_FAMILIES: &[&str] = &[
    "CaskaydiaCove Nerd Font Mono",
    "CaskaydiaMono Nerd Font Mono",
    "JetBrainsMono Nerd Font Mono",
    "FiraCode Nerd Font Mono",
    "Hack Nerd Font Mono",
    "MesloLGM Nerd Font Mono",
    "UbuntuMono Nerd Font Mono",
    "CaskaydiaCove NF",
];

unsafe fn create_font(face: &str, size: i32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            if bold { FW_BOLD } else { FW_NORMAL } as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            FIXED_PITCH as u32,
            wide(face).as_ptr(),
        )
    }
}

unsafe fn resolved_font_face(font: HFONT) -> String {
    unsafe {
        let dc = GetDC(null_mut());
        if dc.is_null() {
            return String::new();
        }

        let old = SelectObject(dc, font);
        let mut buffer = [0u16; 128];
        let len = GetTextFaceW(dc, buffer.len() as i32, buffer.as_mut_ptr());

        SelectObject(dc, old);
        ReleaseDC(null_mut(), dc);

        if len <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..len as usize])
                .trim_end_matches('\0')
                .to_owned()
        }
    }
}

unsafe fn make_terminal_fonts(size: i32) -> (HFONT, HFONT, String) {
    unsafe {
        for family in NERD_FONT_FAMILIES {
            let font = create_font(family, size, false);
            if font.is_null() {
                continue;
            }

            let resolved = resolved_font_face(font);
            if resolved.eq_ignore_ascii_case(family)
                || resolved.to_ascii_lowercase().contains("nerd font")
                || resolved.to_ascii_lowercase().ends_with(" nf")
            {
                let bold = create_font(family, size, true);
                return (font, bold, (*family).to_owned());
            }

            DeleteObject(font);
        }

        for fallback in ["Cascadia Mono", "Consolas"] {
            let font = create_font(fallback, size, false);
            if font.is_null() {
                continue;
            }
            let bold = create_font(fallback, size, true);
            return (font, bold, fallback.to_owned());
        }

        let font = create_font("Consolas", size, false);
        let bold = create_font("Consolas", size, true);
        (font, bold, "Consolas".to_owned())
    }
}

fn color(value: vt100::Color, default: u32) -> u32 {
    const COLORS: [u32; 16] = [0x181512, 0x6B6BF2, 0x86C7A3, 0x83CCE8,
        0xE0AD82, 0xCE9FC9, 0xD7CDC0, 0xDEDAD5, 0x756E67, 0x8787FF,
        0xA8EBC4, 0xA6E8FF, 0xFFD1A8, 0xF0C1EB, 0xF9EFE2, 0xFFFFFF];
    let rgb = |r: u8, g: u8, b: u8| r as u32 | ((g as u32) << 8) | ((b as u32) << 16);
    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => COLORS[i as usize],
        vt100::Color::Idx(i) if i >= 232 => { let v = 8 + (i - 232) * 10; rgb(v, v, v) }
        vt100::Color::Idx(i) => {
            let i = i - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            rgb(component(i / 36), component(i / 6 % 6), component(i % 6))
        }
    }
}

impl Terminal {
    fn input(&mut self, bytes: &[u8]) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink = Instant::now();
        let _ = self.pty.write(bytes);
    }
    fn cell_at(&self, lp: LPARAM) -> usize {
        let x = lp as i16 as i32;
        let y = (lp >> 16) as i16 as i32;
        let (rows, cols) = self.parser.screen().size();
        let col = ((x - PAD).max(0) / self.cell_width).min(cols as i32 - 1);
        let row = ((y - PAD).max(0) / self.cell_height).min(rows as i32 - 1);
        (row * cols as i32 + col) as usize
    }
    fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(a, b)| a != b)
    }

    fn selected_text(&self) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let Some((a, b)) = self.selection else { return screen.contents(); };
        let (start, end) = (a.min(b), a.max(b));
        let mut lines = Vec::new();
        for row in 0..rows {
            let mut line = String::new();
            let mut selected = false;
            for col in 0..cols {
                let index = row as usize * cols as usize + col as usize;
                if index >= start && index <= end {
                    selected = true;
                    if let Some(cell) = screen.cell(row, col) {
                        if cell.is_wide_continuation() { continue; }
                        let text = cell.contents();
                        if text.is_empty() { line.push(' '); } else { line.push_str(text); }
                    }
                }
            }
            if selected { lines.push(line.trim_end().to_owned()); }
        }
        lines.join("\r\n")
    }
    unsafe fn paint(&self, hwnd: HWND) {
        unsafe {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut bounds: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut bounds);
            let mem = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, bounds.right.max(1), bounds.bottom.max(1));
            let old_bitmap = SelectObject(mem, bitmap);
            let brush = CreateSolidBrush(BG);
            FillRect(mem, &bounds, brush);
            DeleteObject(brush);
            let old_font = SelectObject(mem, self.font);
            SetBkMode(mem, OPAQUE as i32);
            let screen = self.parser.screen();
            let (rows, cols) = screen.size();
            let cursor = screen.cursor_position();
            for row in 0..rows {
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else { continue; };
                    if cell.is_wide_continuation() { continue; }
                    let mut fg = color(cell.fgcolor(), FG);
                    let mut bg = color(cell.bgcolor(), BG);
                    if cell.inverse() { std::mem::swap(&mut fg, &mut bg); }
                    let index = row as usize * cols as usize + col as usize;
                    if self.selection.is_some_and(|(a,b)| index >= a.min(b) && index <= a.max(b)) {
                        fg = 0xFFFFFF; bg = 0x706045;
                    }
                    if screen.scrollback() == 0 && !screen.hide_cursor() && self.cursor_on && cursor == (row, col) {
                        fg = BG; bg = 0x86C7A3;
                    }
                    let rect = RECT { left: PAD + col as i32 * self.cell_width, top: PAD + row as i32 * self.cell_height,
                        right: PAD + (col as i32 + if cell.is_wide() { 2 } else { 1 }) * self.cell_width,
                        bottom: PAD + (row as i32 + 1) * self.cell_height };
                    let content = cell.contents();
                    let text = wide(if content.is_empty() { " " } else { content });
                    SetTextColor(mem, fg); SetBkColor(mem, bg);
                    SelectObject(mem, if cell.bold() { self.bold } else { self.font });
                    ExtTextOutW(mem, rect.left, rect.top, ETO_OPAQUE | ETO_CLIPPED, &rect,
                        text.as_ptr(), (text.len() - 1) as u32, null());
                }
            }
            SelectObject(mem, old_font);
            BitBlt(dc, 0, 0, bounds.right, bounds.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bitmap); DeleteObject(bitmap); DeleteDC(mem);
            EndPaint(hwnd, &paint);
        }
    }
}

unsafe fn copy(hwnd: HWND, text: &str) {
    unsafe {
        if OpenClipboard(hwnd) == 0 { return; }
        let text = wide(text);
        let memory = GlobalAlloc(GMEM_MOVEABLE, text.len() * 2);
        if !memory.is_null() {
            let data = GlobalLock(memory);
            if !data.is_null() {
                std::ptr::copy_nonoverlapping(text.as_ptr(), data.cast(), text.len());
                GlobalUnlock(memory);
                EmptyClipboard();
                if SetClipboardData(13, memory).is_null() { GlobalFree(memory); }
            } else { GlobalFree(memory); }
        }
        CloseClipboard();
    }
}

unsafe fn paste(hwnd: HWND, state: &mut Terminal) {
    unsafe {
        if OpenClipboard(hwnd) == 0 { return; }
        let handle = GetClipboardData(13);
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let max_len = GlobalSize(handle) / 2;
                let slice = std::slice::from_raw_parts(ptr, max_len);
                let len = slice.iter().position(|c| *c == 0).unwrap_or(max_len);
                let text = String::from_utf16_lossy(&slice[..len]);
                GlobalUnlock(handle);
                let text = text.replace("\r\n", "\r").replace('\n', "\r");
                if state.parser.screen().bracketed_paste() {
                    state.input(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                } else { state.input(text.as_bytes()); }
            }
        }
        CloseClipboard();
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lp as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Terminal;
        if ptr.is_null() { return DefWindowProcW(hwnd, msg, wp, lp); }
        let state = &mut *ptr;
        match msg {
            WM_CREATE => {
                let dc = GetDC(hwnd);
                let old = SelectObject(dc, state.font);
                let mut metrics: TEXTMETRICW = std::mem::zeroed();
                GetTextMetricsW(dc, &mut metrics);
                state.cell_width = metrics.tmAveCharWidth.max(1);
                state.cell_height = (metrics.tmHeight + 2).max(1);
                SelectObject(dc, old); ReleaseDC(hwnd, dc);
                SetTimer(hwnd, 1, 16, None);
                0
            }
            WM_SIZE => {
                let width = (lp as u32 & 0xffff) as i32;
                let height = ((lp as u32 >> 16) & 0xffff) as i32;
                let cols = ((width - 2 * PAD) / state.cell_width).clamp(2, 500) as u16;
                let rows = ((height - 2 * PAD) / state.cell_height).clamp(2, 200) as u16;
                state.parser.screen_mut().set_size(rows, cols);
                state.pty.resize(cols, rows);
                state.selection = None;
                InvalidateRect(hwnd, null(), 0); 0
            }
            WM_GETMINMAXINFO => {
                (*(lp as *mut MINMAXINFO)).ptMinTrackSize = POINT { x: 400, y: 250 }; 0
            }
            WM_TIMER => {
                let mut changed = false;
                while let Ok(data) = state.pty.output.try_recv() {
                    state.parser.process(&data); changed = true;
                }
                if state.blink.elapsed() >= Duration::from_millis(550) {
                    state.cursor_on = !state.cursor_on; state.blink = Instant::now(); changed = true;
                }
                if changed { InvalidateRect(hwnd, null(), 0); }
                if state.pty.exited() { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
                0
            }
            WM_KEYDOWN => {
                let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                let shift = GetKeyState(VK_SHIFT as i32) < 0;
                if ((ctrl && wp == b'C' as usize) || (ctrl && wp == VK_INSERT as usize))
                    && state.has_selection()
                {
                    copy(hwnd, &state.selected_text());
                    state.suppress_char = true;
                    return 0;
                }
                if (ctrl && shift && wp == b'C' as usize {
                    copy(hwnd, &state.selected_text());
                    state.suppress_char = true;
                    return 0;
                }
                if (ctrl && shift && wp == b'V' as usize) || (shift && wp == VK_INSERT as usize) {
                    paste(hwnd, state); state.suppress_char = true; return 0;
                }
                let sequence = match wp as u16 {
                    VK_UP => if state.parser.screen().application_cursor() { "\x1bOA" } else { "\x1b[A" },
                    VK_DOWN => if state.parser.screen().application_cursor() { "\x1bOB" } else { "\x1b[B" },
                    VK_RIGHT => "\x1b[C", VK_LEFT => "\x1b[D", VK_HOME => "\x1b[H", VK_END => "\x1b[F",
                    VK_DELETE => "\x1b[3~", VK_INSERT => "\x1b[2~", VK_PRIOR => "\x1b[5~", VK_NEXT => "\x1b[6~",
                    _ => return DefWindowProcW(hwnd, msg, wp, lp),
                };
                state.input(sequence.as_bytes()); 0
            }
            WM_CHAR => {
                if state.suppress_char { state.suppress_char = false; return 0; }
                let ch = wp as u16;
                if (0xD800..=0xDBFF).contains(&ch) { state.surrogate = Some(ch); return 0; }
                let text = if let Some(high) = state.surrogate.take() { String::from_utf16_lossy(&[high, ch]) }
                    else if ch == 8 { "\x7f".to_owned() } else { String::from_utf16_lossy(&[ch]) };
                state.input(text.as_bytes()); 0
            }
            WM_LBUTTONDOWN => {
                SetFocus(hwnd); SetCapture(hwnd);
                let index = state.cell_at(lp); state.selection = Some((index, index)); state.dragging = true;
                InvalidateRect(hwnd, null(), 0); 0
            }
            WM_MOUSEMOVE if state.dragging => {
                let index = state.cell_at(lp);
                if let Some((a, _)) = state.selection { state.selection = Some((a, index)); }
                InvalidateRect(hwnd, null(), 0); 0
            }
            WM_LBUTTONUP => { state.dragging = false; ReleaseCapture(); 0 }
            WM_RBUTTONUP => { paste(hwnd, state); 0 }
            WM_MOUSEWHEEL => {
                let delta = ((wp >> 16) as i16) as i32 / 120;
                let scroll = state.parser.screen().scrollback() as i32;
                state.parser.screen_mut().set_scrollback((scroll + delta * 3).max(0) as usize);
                state.selection = None; InvalidateRect(hwnd, null(), 0); 0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT => { state.paint(hwnd); 0 }
            WM_DESTROY => { KillTimer(hwnd, 1); PostQuitMessage(0); 0 }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let state = Box::from_raw(ptr);
                DeleteObject(state.font); DeleteObject(state.bold);
                drop(state);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub fn verify_transport() -> Result<()> {
    let mut pty = EmbeddedSession::start(100, 30)?;
    let mut parser = vt100::Parser::new(30, 100, 1000);
    let started = Instant::now();
    let mut sent = false;
    loop {
        if started.elapsed() > Duration::from_secs(15) { bail!("El intérprete no respondió. Pantalla: {}", parser.screen().contents()); }
        if let Ok(bytes) = pty.output.recv_timeout(Duration::from_millis(100)) { parser.process(&bytes); }
        let contents = parser.screen().contents();
        if !sent && contents.contains("$ ") { pty.write(b"echo ADM_NATIVE_OK\r")?; sent = true; }
        if sent && contents.matches("ADM_NATIVE_OK").count() >= 2 {
            pty.write(b"exit\r")?;
            println!("Terminal propia: prompt, entrada, ejecución y salida correctos.");
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn vt_screen_supports_colors_cursor_and_alternate_screen() {
        let mut parser = vt100::Parser::new(10, 40, 100);
        parser.process(b"\x1b[32mhello\x1b[0m\r\nworld");
        assert_eq!(parser.screen().cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(2));
        assert_eq!(parser.screen().cursor_position(), (1, 5));
        parser.process(b"\x1b[?1049h\x1b[2Jeditor");
        assert!(parser.screen().alternate_screen());
        parser.process(b"\x1b[?1049l");
        assert!(parser.screen().contents().contains("hello"));
    }
}
