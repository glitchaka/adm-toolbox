//! Shell Shock Tool native terminal: custom renderer, embedded Nerd Font and portable shell.
use std::{
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use anyhow::{bail, Result};
use crossterm::event::{KeyCode as CtKeyCode, KeyEvent as CtKeyEvent, KeyModifiers as CtKeyModifiers};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::{Controls::MARGINS, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use crate::adapters::terminal::embedded::EmbeddedSession;

const PAD: i32 = 14;
const TITLE_BAR_HEIGHT: i32 = 44;
const RESIZE_BORDER: i32 = 7;
const WINDOW_WIDTH: i32 = 1240;
const WINDOW_HEIGHT: i32 = 820;

const NERD_FONT_FAMILY: &str = "JetBrainsMono Nerd Font Mono";
static NERD_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/JetBrainsMonoNerdFontMono-Regular.ttf"
));

const BG: u32 = 0x291611;          // #111629
const TITLE_BG: u32 = 0x140D0A;    // #0A0D14
const FG: u32 = 0xEFE8DF;          // #DFE8EF
const ACCENT_BLUE: u32 = 0xFFC769; // #69C7FF
const ACCENT_PINK: u32 = 0xBD9FFF; // #FF9FBD
const BTN_YELLOW: u32 = 0x1BC6F6;  // #F6C61B
const BTN_BLUE: u32 = 0xF5A81F;    // #1FA8F5
const BTN_RED: u32 = 0x382DFF;     // #FF2D38

#[derive(Clone, Copy, PartialEq, Eq)]
enum TitleButton {
    Minimize,
    Maximize,
    Close,
}

struct Terminal {
    pty: EmbeddedSession,
    parser: vt100::Parser,
    font: HFONT,
    bold: HFONT,
    icon: HICON,
    cell_width: i32,
    cell_height: i32,
    selection: Option<(usize, usize)>,
    dragging: bool,
    hovered_title_button: Option<TitleButton>,
    pressed_title_button: Option<TitleButton>,
    suppress_char: bool,
    surrogate: Option<u16>,
    cursor_on: bool,
    blink: Instant,
    font_resource: HANDLE,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn run() -> Result<()> {
    unsafe {
        SetProcessDPIAware();

        let instance = GetModuleHandleW(null());
        let class = wide("AdmToolboxTerminal");
        let icon = LoadIconW(instance, 1usize as *const u16);

        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_IBEAM),
            hIcon: icon,
            ..std::mem::zeroed()
        };

        if RegisterClassW(&wc) == 0 {
            bail!(
                "No se pudo registrar la terminal: {}",
                std::io::Error::last_os_error()
            );
        }

        let pty = EmbeddedSession::start(112, 31)?;
        let mut loaded_fonts = 0u32;
        let font_resource = AddFontMemResourceEx(
            NERD_FONT_BYTES.as_ptr().cast(),
            NERD_FONT_BYTES.len() as u32,
            null_mut(),
            &mut loaded_fonts,
        );

        if font_resource.is_null() || loaded_fonts == 0 {
            bail!("No se pudo cargar la Nerd Font embebida");
        }

        let font = create_font(NERD_FONT_FAMILY, 19, false);
        let bold = create_font(NERD_FONT_FAMILY, 19, true);

        if font.is_null() || bold.is_null() {
            if !font.is_null() {
                DeleteObject(font);
            }
            if !bold.is_null() {
                DeleteObject(bold);
            }
            RemoveFontMemResourceEx(font_resource);
            bail!("No se pudo crear la tipografía Nerd Font embebida");
        }

        let state = Box::new(Terminal {
            pty,
            parser: vt100::Parser::new(31, 112, 10_000),
            font,
            bold,
            icon,
            cell_width: 10,
            cell_height: 23,
            selection: None,
            dragging: false,
            hovered_title_button: None,
            pressed_title_button: None,
            suppress_char: false,
            surrogate: None,
            cursor_on: true,
            blink: Instant::now(),
            font_resource,
        });

        let ptr = Box::into_raw(state);
        let style = WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU;
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class.as_ptr(),
            wide("Shell Shock Tool").as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            ptr.cast(),
        );

        if hwnd.is_null() {
            let state = Box::from_raw(ptr);
            DeleteObject(state.font);
            DeleteObject(state.bold);
            RemoveFontMemResourceEx(state.font_resource);
            bail!(
                "No se pudo crear la terminal: {}",
                std::io::Error::last_os_error()
            );
        }

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result == -1 {
                bail!("Error en el bucle de ventanas");
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

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

unsafe fn apply_window_effects(hwnd: HWND) {
    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20,
            (&dark as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        // Do not use DWMWA_SYSTEMBACKDROP_TYPE here. It affects the entire client
        // window, including the custom titlebar, and Windows changes its tint when
        // the window becomes inactive. The terminal glass is instead created by
        // extending only the bottom DWM frame into the client area.
        let backdrop_none: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38,
            (&backdrop_none as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        let corners: i32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            33,
            (&corners as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        update_terminal_glass_region(hwnd);
    }
}

unsafe fn update_terminal_glass_region(hwnd: HWND) {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);

        // Extend DWM glass upward from the bottom edge only as far as the terminal.
        // The custom titlebar remains outside the extended frame, so it is always
        // rendered as a normal opaque client surface.
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 0,
            cyBottomHeight: (client.bottom - TITLE_BAR_HEIGHT).max(0),
        };

        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}

fn color(value: vt100::Color, default: u32) -> u32 {
    const COLORS: [u32; 16] = [
        0x291611, 0x6B6BF2, 0x86C7A3, 0x83CCE8,
        0xE0AD82, 0xCE9FC9, 0xD7CDC0, 0xEFE8DF,
        0x756E67, 0x8787FF, 0xA8EBC4, 0xA6E8FF,
        0xFFD1A8, 0xF0C1EB, 0xF9EFE2, 0xFFFFFF,
    ];

    let rgb = |r: u8, g: u8, b: u8| r as u32 | ((g as u32) << 8) | ((b as u32) << 16);

    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => COLORS[i as usize],
        vt100::Color::Idx(i) if i >= 232 => {
            let v = 8 + (i - 232) * 10;
            rgb(v, v, v)
        }
        vt100::Color::Idx(i) => {
            let i = i - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            rgb(component(i / 36), component(i / 6 % 6), component(i % 6))
        }
    }
}

fn point_from_lparam(lp: LPARAM) -> (i32, i32) {
    (lp as i16 as i32, (lp >> 16) as i16 as i32)
}

unsafe fn screen_point_to_client(hwnd: HWND, x: i32, y: i32) -> (i32, i32) {
    unsafe {
        let mut point = POINT { x, y };
        ScreenToClient(hwnd, &mut point);
        (point.x, point.y)
    }
}

fn title_button_at(hwnd: HWND, x: i32, y: i32) -> Option<TitleButton> {
    if !(0..TITLE_BAR_HEIGHT).contains(&y) {
        return None;
    }

    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);
        let right = rect.right;

        [
            (TitleButton::Minimize, right - 109, right - 75),
            (TitleButton::Maximize, right - 75, right - 41),
            (TitleButton::Close, right - 41, right - 7),
        ]
        .into_iter()
        .find_map(|(button, left, right)| {
            if x >= left && x < right {
                Some(button)
            } else {
                None
            }
        })
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
        let (x, y) = point_from_lparam(lp);
        let (rows, cols) = self.parser.screen().size();
        let col = ((x - PAD).max(0) / self.cell_width).min(cols as i32 - 1);
        let content_y = (y - TITLE_BAR_HEIGHT - PAD).max(0);
        let row = (content_y / self.cell_height).min(rows as i32 - 1);
        (row * cols as i32 + col) as usize
    }

    fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(a, b)| a != b)
    }

    fn selected_text(&self) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let Some((a, b)) = self.selection else {
            return screen.contents();
        };

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
                        if cell.is_wide_continuation() {
                            continue;
                        }

                        let text = cell.contents();
                        if text.is_empty() {
                            line.push(' ');
                        } else {
                            line.push_str(text);
                        }
                    }
                }
            }

            if selected {
                lines.push(line.trim_end().to_owned());
            }
        }

        lines.join("\r\n")
    }

    unsafe fn paint_titlebar(&self, _hwnd: HWND, dc: HDC, bounds: &RECT) {
        unsafe {
            let title_rect = RECT {
                left: 0,
                top: 0,
                right: bounds.right,
                bottom: TITLE_BAR_HEIGHT,
            };
            let brush = CreateSolidBrush(TITLE_BG);
            FillRect(dc, &title_rect, brush);
            DeleteObject(brush);

            if !self.icon.is_null() {
                DrawIconEx(dc, 12, 8, self.icon, 28, 28, 0, null_mut(), DI_NORMAL);
            }

            let old_font = SelectObject(dc, self.bold);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, FG);

            let title = wide("Shell Shock Tool");
            TextOutW(dc, 50, 12, title.as_ptr(), (title.len() - 1) as i32);

            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            for (button_kind, cx, color) in [
                (TitleButton::Minimize, bounds.right - 92, BTN_YELLOW),
                (TitleButton::Maximize, bounds.right - 58, BTN_BLUE),
                (TitleButton::Close, bounds.right - 24, BTN_RED),
            ] {
                let radius = if self.pressed_title_button == Some(button_kind) {
                    8
                } else if self.hovered_title_button == Some(button_kind) {
                    11
                } else {
                    10
                };
                let button = CreateSolidBrush(color);
                let old_brush = SelectObject(dc, button);
                Ellipse(dc, cx - radius, 22 - radius, cx + radius, 22 + radius);
                SelectObject(dc, old_brush);
                DeleteObject(button);

                if self.hovered_title_button == Some(button_kind) {
                    let glyph_pen = CreatePen(PS_SOLID, 2, TITLE_BG);
                    let old_glyph_pen = SelectObject(dc, glyph_pen);
                    match button_kind {
                        TitleButton::Minimize => {
                            MoveToEx(dc, cx - 4, 22, null_mut());
                            LineTo(dc, cx + 5, 22);
                        }
                        TitleButton::Maximize => {
                            Rectangle(dc, cx - 4, 18, cx + 5, 27);
                        }
                        TitleButton::Close => {
                            MoveToEx(dc, cx - 4, 18, null_mut());
                            LineTo(dc, cx + 5, 27);
                            MoveToEx(dc, cx + 4, 18, null_mut());
                            LineTo(dc, cx - 5, 27);
                        }
                    }
                    SelectObject(dc, old_glyph_pen);
                    DeleteObject(glyph_pen);
                }
            }
            SelectObject(dc, old_pen);
            SelectObject(dc, old_font);
        }
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

            // Black is the DWM glass key only inside the lower extended frame.
            // The titlebar is repainted afterward with TITLE_BG and sits outside
            // that frame, so it remains fully opaque.
            let glass = CreateSolidBrush(0x000000);
            FillRect(mem, &bounds, glass);
            DeleteObject(glass);

            self.paint_titlebar(hwnd, mem, &bounds);

            let old_font = SelectObject(mem, self.font);
            SetBkMode(mem, TRANSPARENT as i32);

            let screen = self.parser.screen();
            let (rows, cols) = screen.size();
            let cursor = screen.cursor_position();

            for row in 0..rows {
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else {
                        continue;
                    };
                    if cell.is_wide_continuation() {
                        continue;
                    }

                    let mut fg = color(cell.fgcolor(), FG);
                    let mut bg = color(cell.bgcolor(), BG);
                    let mut paint_background =
                        !matches!(cell.bgcolor(), vt100::Color::Default) || cell.inverse();

                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                    }

                    let index = row as usize * cols as usize + col as usize;
                    if self
                        .selection
                        .is_some_and(|(a, b)| index >= a.min(b) && index <= a.max(b))
                    {
                        fg = 0xFFFFFF;
                        bg = 0x704B37;
                        paint_background = true;
                    }

                    if screen.scrollback() == 0
                        && !screen.hide_cursor()
                        && self.cursor_on
                        && cursor == (row, col)
                    {
                        fg = BG;
                        bg = ACCENT_BLUE;
                        paint_background = true;
                    }

                    let rect = RECT {
                        left: PAD + col as i32 * self.cell_width,
                        top: TITLE_BAR_HEIGHT + PAD + row as i32 * self.cell_height,
                        right: PAD
                            + (col as i32 + if cell.is_wide() { 2 } else { 1 })
                                * self.cell_width,
                        bottom: TITLE_BAR_HEIGHT
                            + PAD
                            + (row as i32 + 1) * self.cell_height,
                    };

                    if paint_background {
                        let brush = CreateSolidBrush(bg);
                        FillRect(mem, &rect, brush);
                        DeleteObject(brush);
                    }

                    let content = cell.contents();
                    if content.is_empty() {
                        continue;
                    }
                    let text = wide(content);

                    SetTextColor(mem, fg);
                    SelectObject(mem, if cell.bold() { self.bold } else { self.font });
                    ExtTextOutW(
                        mem,
                        rect.left,
                        rect.top,
                        ETO_CLIPPED,
                        &rect,
                        text.as_ptr(),
                        (text.len() - 1) as u32,
                        null(),
                    );
                }
            }

            // A thin accent line separates the custom titlebar from the terminal.
            let accent = CreateSolidBrush(ACCENT_PINK);
            let line = RECT {
                left: 0,
                top: TITLE_BAR_HEIGHT - 1,
                right: bounds.right,
                bottom: TITLE_BAR_HEIGHT,
            };
            FillRect(mem, &line, accent);
            DeleteObject(accent);

            SelectObject(mem, old_font);
            BitBlt(dc, 0, 0, bounds.right, bounds.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(mem);
            EndPaint(hwnd, &paint);
        }
    }
}

unsafe fn copy(hwnd: HWND, text: &str) {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }

        let text = wide(text);
        let memory = GlobalAlloc(GMEM_MOVEABLE, text.len() * 2);

        if !memory.is_null() {
            let data = GlobalLock(memory);
            if !data.is_null() {
                std::ptr::copy_nonoverlapping(text.as_ptr(), data.cast(), text.len());
                GlobalUnlock(memory);
                EmptyClipboard();

                if SetClipboardData(13, memory).is_null() {
                    GlobalFree(memory);
                }
            } else {
                GlobalFree(memory);
            }
        }

        CloseClipboard();
    }
}

unsafe fn paste(hwnd: HWND, state: &mut Terminal) {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }

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
                } else {
                    state.input(text.as_bytes());
                }
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
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }

        let state = &mut *ptr;

        match msg {
            WM_CREATE => {
                apply_window_effects(hwnd);

                let dc = GetDC(hwnd);
                let old = SelectObject(dc, state.font);
                let mut metrics: TEXTMETRICW = std::mem::zeroed();
                GetTextMetricsW(dc, &mut metrics);
                state.cell_width = metrics.tmAveCharWidth.max(1);
                state.cell_height = (metrics.tmHeight + 2).max(1);
                SelectObject(dc, old);
                ReleaseDC(hwnd, dc);

                SetTimer(hwnd, 1, 16, None);
                0
            }

            WM_NCHITTEST => {
                let (screen_x, screen_y) = point_from_lparam(lp);
                let (x, y) = screen_point_to_client(hwnd, screen_x, screen_y);

                let mut client: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut client);
                let width = client.right;
                let height = client.bottom;

                // Los tres controles tienen prioridad sobre el caption y el borde.
                // Sus posiciones se dibujan en coordenadas de cliente, por lo que
                // el hit-test debe usar exactamente el mismo espacio.
                if title_button_at(hwnd, x, y).is_some() {
                    return HTCLIENT as isize;
                }

                let left = x < RESIZE_BORDER;
                let right = x >= width - RESIZE_BORDER;
                let top = y < RESIZE_BORDER;
                let bottom = y >= height - RESIZE_BORDER;

                if top && left {
                    return HTTOPLEFT as isize;
                }
                if top && right {
                    return HTTOPRIGHT as isize;
                }
                if bottom && left {
                    return HTBOTTOMLEFT as isize;
                }
                if bottom && right {
                    return HTBOTTOMRIGHT as isize;
                }
                if left {
                    return HTLEFT as isize;
                }
                if right {
                    return HTRIGHT as isize;
                }
                if top {
                    return HTTOP as isize;
                }
                if bottom {
                    return HTBOTTOM as isize;
                }

                if (0..TITLE_BAR_HEIGHT).contains(&y) {
                    return HTCAPTION as isize;
                }

                HTCLIENT as isize
            }

            WM_SIZE => {
                let width = (lp as u32 & 0xffff) as i32;
                let height = ((lp as u32 >> 16) & 0xffff) as i32;
                let cols = ((width - 2 * PAD) / state.cell_width).clamp(2, 500) as u16;
                let rows = ((height - TITLE_BAR_HEIGHT - 2 * PAD) / state.cell_height)
                    .clamp(2, 200) as u16;

                state.parser.screen_mut().set_size(rows, cols);
                state.pty.resize(cols, rows);
                update_terminal_glass_region(hwnd);
                state.selection = None;
                InvalidateRect(hwnd, null(), 0);
                0
            }

            WM_GETMINMAXINFO => {
                (*(lp as *mut MINMAXINFO)).ptMinTrackSize = POINT { x: 520, y: 320 };
                0
            }

            WM_TIMER => {
                let mut changed = false;

                while let Ok(data) = state.pty.output.try_recv() {
                    state.parser.process(&data);
                    changed = true;
                }

                if state.blink.elapsed() >= Duration::from_millis(550) {
                    state.cursor_on = !state.cursor_on;
                    state.blink = Instant::now();
                    changed = true;
                }

                if changed {
                    InvalidateRect(hwnd, null(), 0);
                }

                state.pty.check_timeout();

                if state.pty.exited() {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }

                0
            }

            WM_KEYDOWN => {
                let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                let shift = GetKeyState(VK_SHIFT as i32) < 0;

                // Ctrl+Q is reserved by Shell Shock Tool as a hard abort.
                // It must never be forwarded to the foreground application.
                if ctrl && wp == b'Q' as usize {
                    state.pty.force_abort();
                    state.suppress_char = true;
                    return 0;
                }

                if state.pty.raw_mode() {
                    if ((ctrl && wp == b'C' as usize) || (ctrl && wp == VK_INSERT as usize))
                        && state.has_selection()
                    {
                        copy(hwnd, &state.selected_text());
                        state.suppress_char = true;
                        return 0;
                    }

                    if (ctrl && wp == b'V' as usize) || (shift && wp == VK_INSERT as usize) {
                        paste(hwnd, state);
                        state.suppress_char = true;
                        return 0;
                    }

                    let mut modifiers = CtKeyModifiers::NONE;
                    if ctrl {
                        modifiers |= CtKeyModifiers::CONTROL;
                    }
                    if shift {
                        modifiers |= CtKeyModifiers::SHIFT;
                    }

                    let code = match wp as u16 {
                        VK_ESCAPE => Some(CtKeyCode::Esc),
                        VK_RETURN => Some(CtKeyCode::Enter),
                        VK_TAB if shift => Some(CtKeyCode::BackTab),
                        VK_TAB => Some(CtKeyCode::Tab),
                        VK_BACK => Some(CtKeyCode::Backspace),
                        VK_UP => Some(CtKeyCode::Up),
                        VK_DOWN => Some(CtKeyCode::Down),
                        VK_LEFT => Some(CtKeyCode::Left),
                        VK_RIGHT => Some(CtKeyCode::Right),
                        VK_HOME => Some(CtKeyCode::Home),
                        VK_END => Some(CtKeyCode::End),
                        VK_DELETE => Some(CtKeyCode::Delete),
                        VK_INSERT => Some(CtKeyCode::Insert),
                        VK_PRIOR => Some(CtKeyCode::PageUp),
                        VK_NEXT => Some(CtKeyCode::PageDown),
                        _ if ctrl && wp >= b'A' as usize && wp <= b'Z' as usize => {
                            Some(CtKeyCode::Char((wp as u8).to_ascii_lowercase() as char))
                        }
                        _ => None,
                    };

                    if let Some(code) = code {
                        let _ = state.pty.send_raw_key(CtKeyEvent::new(code, modifiers));
                        state.suppress_char = matches!(
                            code,
                            CtKeyCode::Esc
                                | CtKeyCode::Enter
                                | CtKeyCode::Tab
                                | CtKeyCode::BackTab
                                | CtKeyCode::Backspace
                                | CtKeyCode::Char(_)
                        );
                        return 0;
                    }
                }

                if ((ctrl && wp == b'C' as usize) || (ctrl && wp == VK_INSERT as usize))
                    && state.has_selection()
                {
                    copy(hwnd, &state.selected_text());
                    state.suppress_char = true;
                    return 0;
                }

                if ctrl && shift && wp == b'C' as usize {
                    copy(hwnd, &state.selected_text());
                    state.suppress_char = true;
                    return 0;
                }

                if (ctrl && wp == b'V' as usize) || (shift && wp == VK_INSERT as usize) {
                    paste(hwnd, state);
                    state.suppress_char = true;
                    return 0;
                }

                let sequence = match wp as u16 {
                    VK_UP => {
                        if state.parser.screen().application_cursor() {
                            "\x1bOA"
                        } else {
                            "\x1b[A"
                        }
                    }
                    VK_DOWN => {
                        if state.parser.screen().application_cursor() {
                            "\x1bOB"
                        } else {
                            "\x1b[B"
                        }
                    }
                    VK_RIGHT => "\x1b[C",
                    VK_LEFT => "\x1b[D",
                    VK_HOME => "\x1b[H",
                    VK_END => "\x1b[F",
                    VK_DELETE => "\x1b[3~",
                    VK_INSERT => "\x1b[2~",
                    VK_PRIOR => "\x1b[5~",
                    VK_NEXT => "\x1b[6~",
                    _ => return DefWindowProcW(hwnd, msg, wp, lp),
                };

                state.input(sequence.as_bytes());
                0
            }

            WM_PASTE => {
                paste(hwnd, state);
                0
            }

            WM_CHAR => {
                if state.suppress_char {
                    state.suppress_char = false;
                    return 0;
                }

                let ch = wp as u16;
                if (0xD800..=0xDBFF).contains(&ch) {
                    state.surrogate = Some(ch);
                    return 0;
                }

                let text = if let Some(high) = state.surrogate.take() {
                    String::from_utf16_lossy(&[high, ch])
                } else if ch == 8 {
                    "\x7f".to_owned()
                } else {
                    String::from_utf16_lossy(&[ch])
                };

                state.input(text.as_bytes());
                0
            }

            WM_LBUTTONDOWN => {
                let (x, y) = point_from_lparam(lp);
                SetFocus(hwnd);

                if let Some(button) = title_button_at(hwnd, x, y) {
                    state.pressed_title_button = Some(button);
                    state.hovered_title_button = Some(button);
                    state.dragging = false;
                    SetCapture(hwnd);
                    InvalidateRect(hwnd, null(), 0);
                    return 0;
                }

                if y < TITLE_BAR_HEIGHT {
                    return DefWindowProcW(hwnd, msg, wp, lp);
                }

                SetCapture(hwnd);
                let index = state.cell_at(lp);
                state.selection = Some((index, index));
                state.dragging = true;
                InvalidateRect(hwnd, null(), 0);
                0
            }

            WM_MOUSEMOVE => {
                let (x, y) = point_from_lparam(lp);

                let hovered = title_button_at(hwnd, x, y);
                if state.hovered_title_button != hovered {
                    state.hovered_title_button = hovered;
                    InvalidateRect(hwnd, null(), 0);
                }

                if state.dragging {
                    let index = state.cell_at(lp);
                    if let Some((a, _)) = state.selection {
                        state.selection = Some((a, index));
                    }
                    InvalidateRect(hwnd, null(), 0);
                }

                0
            }

            WM_LBUTTONUP => {
                let (x, y) = point_from_lparam(lp);
                let released_over = title_button_at(hwnd, x, y);
                let pressed = state.pressed_title_button.take();

                if pressed.is_some() {
                    ReleaseCapture();
                    InvalidateRect(hwnd, null(), 0);

                    if pressed == released_over {
                        match pressed.unwrap() {
                            TitleButton::Minimize => {
                                ShowWindow(hwnd, SW_MINIMIZE);
                            }
                            TitleButton::Maximize => {
                                if IsZoomed(hwnd) != 0 {
                                    ShowWindow(hwnd, SW_RESTORE);
                                } else {
                                    ShowWindow(hwnd, SW_MAXIMIZE);
                                }
                            }
                            TitleButton::Close => {
                                SendMessageW(hwnd, WM_CLOSE, 0, 0);
                            }
                        }
                    }
                    return 0;
                }

                state.dragging = false;
                ReleaseCapture();
                0
            }

            WM_CAPTURECHANGED => {
                state.pressed_title_button = None;
                state.dragging = false;
                InvalidateRect(hwnd, null(), 0);
                0
            }

            WM_RBUTTONUP => {
                let (_, y) = point_from_lparam(lp);
                if y >= TITLE_BAR_HEIGHT {
                    paste(hwnd, state);
                }
                0
            }

            WM_MOUSEWHEEL => {
                let delta = ((wp >> 16) as i16) as i32 / 120;
                let scroll = state.parser.screen().scrollback() as i32;
                state
                    .parser
                    .screen_mut()
                    .set_scrollback((scroll + delta * 3).max(0) as usize);
                state.selection = None;
                InvalidateRect(hwnd, null(), 0);
                0
            }

            WM_SETCURSOR => {
                let mut point: POINT = std::mem::zeroed();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);

                let cursor = if point.y < TITLE_BAR_HEIGHT {
                    LoadCursorW(null_mut(), IDC_ARROW)
                } else {
                    LoadCursorW(null_mut(), IDC_IBEAM)
                };
                SetCursor(cursor);
                1
            }

            WM_ERASEBKGND => 1,

            WM_PAINT => {
                state.paint(hwnd);
                0
            }

            WM_DESTROY => {
                KillTimer(hwnd, 1);
                PostQuitMessage(0);
                0
            }

            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let state = Box::from_raw(ptr);
                DeleteObject(state.font);
                DeleteObject(state.bold);
                RemoveFontMemResourceEx(state.font_resource);
                drop(state);
                DefWindowProcW(hwnd, msg, wp, lp)
            }

            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub fn verify_transport() -> Result<()> {
    let mut pty = EmbeddedSession::start(112, 31)?;
    let mut parser = vt100::Parser::new(31, 112, 1000);
    let started = Instant::now();
    let mut sent = false;

    loop {
        if started.elapsed() > Duration::from_secs(15) {
            bail!(
                "El intérprete no respondió. Pantalla: {}",
                parser.screen().contents()
            );
        }

        if let Ok(bytes) = pty.output.recv_timeout(Duration::from_millis(100)) {
            parser.process(&bytes);
        }

        let contents = parser.screen().contents();
        if !sent && (contents.contains("❯") || contents.contains("$ ")) {
            pty.write(b"echo ADM_NATIVE_OK\r")?;
            sent = true;
        }

        if sent && contents.matches("ADM_NATIVE_OK").count() >= 2 {
            pty.write(b"exit\r")?;
            println!("Shell Shock Tool: prompt, entrada, ejecución y salida correctos.");
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
        assert_eq!(
            parser.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(2)
        );
        assert_eq!(parser.screen().cursor_position(), (1, 5));
        parser.process(b"\x1b[?1049h\x1b[2Jeditor");
        assert!(parser.screen().alternate_screen());
        parser.process(b"\x1b[?1049l");
        assert!(parser.screen().contents().contains("hello"));
    }
}
