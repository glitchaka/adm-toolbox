//! Shell Shock Tool native terminal: custom renderer, embedded Nerd Font and portable shell.
use std::{
    ffi::c_void,
    fs,
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use chrono::Local;
use serde::Deserialize;
use sysinfo::System;
use crossterm::event::{KeyCode as CtKeyCode, KeyEvent as CtKeyEvent, KeyModifiers as CtKeyModifiers};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    System::{DataExchange::*, LibraryLoader::{GetModuleHandleW, GetProcAddress}, Memory::*},
    UI::{Controls::MARGINS, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

use crate::adapters::{
    persistence::AppPaths,
    terminal::embedded::EmbeddedSession,
};

const PAD: i32 = 14;
const TITLE_BAR_HEIGHT: i32 = 32;
const TITLE_ISLAND_WIDTH: i32 = 760;
const TITLE_ISLAND_TOP: i32 = 6;
const TITLE_ISLAND_RADIUS: i32 = 12;
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
const TITLE_CONTROL: u32 = 0xBD9FFF; // #FF9FBD
const TITLE_CLOSE: u32 = 0x382DFF;   // #FF2D38
const TITLE_HOVER_BG: u32 = 0x211B18;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct TerminalAppearance {
    backdrop: String,
    background_opacity: u8,
    background_color: String,
}

impl Default for TerminalAppearance {
    fn default() -> Self {
        Self {
            backdrop: "acrylic".to_owned(),
            background_opacity: 82,
            background_color: "#111629".to_owned(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct TerminalConfig {
    #[serde(default)]
    appearance: TerminalAppearance,
}

#[repr(C)]
struct AccentPolicy {
    accent_state: i32,
    accent_flags: i32,
    gradient_color: u32,
    animation_id: i32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: i32,
    data: *mut c_void,
    size_of_data: usize,
}

const WCA_ACCENT_POLICY: i32 = 19;
const ACCENT_DISABLED: i32 = 0;
const ACCENT_ENABLE_BLUR_BEHIND: i32 = 3;
const ACCENT_ENABLE_ACRYLIC_BLUR_BEHIND: i32 = 4;

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
    appearance: TerminalAppearance,
    metrics: System,
    status_refreshed: Instant,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn load_terminal_appearance(paths: &AppPaths) -> Result<TerminalAppearance> {
    let path = paths.terminal_config_file();
    let text = fs::read_to_string(&path)
        .with_context(|| format!("No se pudo leer {}", path.display()))?;
    let mut config: TerminalConfig = toml::from_str(&text)
        .with_context(|| format!("Configuración visual inválida: {}", path.display()))?;

    config.appearance.background_opacity = config.appearance.background_opacity.min(100);
    config.appearance.backdrop = config.appearance.backdrop.trim().to_ascii_lowercase();

    if !matches!(
        config.appearance.backdrop.as_str(),
        "acrylic" | "blur" | "glass" | "solid"
    ) {
        anyhow::bail!(
            "terminal.toml: appearance.backdrop debe ser acrylic, blur, glass o solid"
        );
    }

    parse_hex_color(&config.appearance.background_color)
        .with_context(|| "terminal.toml: appearance.background_color inválido")?;

    Ok(config.appearance)
}

fn parse_hex_color(value: &str) -> Result<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        anyhow::bail!("el color debe usar formato #RRGGBB");
    }

    let rgb = u32::from_str_radix(hex, 16)?;
    let r = (rgb >> 16) & 0xff;
    let g = (rgb >> 8) & 0xff;
    let b = rgb & 0xff;
    Ok(r | (g << 8) | (b << 16))
}

pub fn run() -> Result<()> {
    let paths = AppPaths::detect();
    paths.ensure_layout()?;
    let appearance = load_terminal_appearance(&paths)?;

    unsafe {
        SetProcessDPIAware();

        let instance = GetModuleHandleW(null());
        let class = wide("ShellShockToolTerminal");
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

        let mut metrics = System::new_all();
        metrics.refresh_all();

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
            appearance,
            metrics,
            status_refreshed: Instant::now(),
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

unsafe fn apply_window_effects(hwnd: HWND, appearance: &TerminalAppearance) {
    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20,
            (&dark as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        // SST controls the terminal backdrop itself. Disable the automatic system
        // backdrop so Windows does not retint the custom titlebar on focus changes.
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

        apply_configured_backdrop(hwnd, appearance);

        if appearance.backdrop != "solid" {
            update_terminal_glass_region(hwnd);
        } else {
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 0,
                cyBottomHeight: 0,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
    }
}

unsafe fn apply_configured_backdrop(hwnd: HWND, appearance: &TerminalAppearance) {
    unsafe {
        let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
        if user32.is_null() {
            return;
        }

        let Some(proc) = GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr()) else {
            return;
        };

        type SetWindowCompositionAttributeFn =
            unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;
        let set_attribute: SetWindowCompositionAttributeFn = std::mem::transmute(proc);

        let state = match appearance.backdrop.as_str() {
            "acrylic" => ACCENT_ENABLE_ACRYLIC_BLUR_BEHIND,
            "blur" => ACCENT_ENABLE_BLUR_BEHIND,
            _ => ACCENT_DISABLED,
        };

        let alpha = ((u32::from(appearance.background_opacity) * 255) / 100) << 24;
        let tint = parse_hex_color(&appearance.background_color).unwrap_or(BG);
        let mut policy = AccentPolicy {
            accent_state: state,
            accent_flags: 2,
            gradient_color: alpha | tint,
            animation_id: 0,
        };
        let mut data = WindowCompositionAttributeData {
            attribute: WCA_ACCENT_POLICY,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size_of_data: size_of::<AccentPolicy>(),
        };

        let _ = set_attribute(hwnd, &mut data);
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

fn title_island_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);
        let available = (client.right - 24).max(220);
        let width = TITLE_ISLAND_WIDTH.min(available);
        let left = ((client.right - width) / 2).max(12);

        RECT {
            left,
            top: TITLE_ISLAND_TOP,
            right: left + width,
            bottom: TITLE_ISLAND_TOP + TITLE_BAR_HEIGHT,
        }
    }
}

fn title_button_at(hwnd: HWND, x: i32, y: i32) -> Option<TitleButton> {
    let island = title_island_rect(hwnd);
    if x < island.left || x >= island.right || y < island.top || y >= island.bottom {
        return None;
    }

    [
        (TitleButton::Minimize, island.right - 102, island.right - 68),
        (TitleButton::Maximize, island.right - 68, island.right - 34),
        (TitleButton::Close, island.right - 34, island.right),
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

    unsafe fn paint_titlebar(&self, hwnd: HWND, dc: HDC, _bounds: &RECT) {
        unsafe {
            let island = title_island_rect(hwnd);

            let brush = CreateSolidBrush(TITLE_BG);
            let old_brush = SelectObject(dc, brush);
            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            RoundRect(
                dc,
                island.left,
                island.top,
                island.right,
                island.bottom,
                TITLE_ISLAND_RADIUS,
                TITLE_ISLAND_RADIUS,
            );
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            DeleteObject(brush);

            if !self.icon.is_null() {
                DrawIconEx(
                    dc,
                    island.left + 10,
                    island.top + 6,
                    self.icon,
                    20,
                    20,
                    0,
                    null_mut(),
                    DI_NORMAL,
                );
            }

            let old_font = SelectObject(dc, self.bold);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, FG);

            let title = wide("SST");
            TextOutW(
                dc,
                island.left + 38,
                island.top + 7,
                title.as_ptr(),
                (title.len() - 1) as i32,
            );

            let cpu = if self.metrics.cpus().is_empty() {
                0.0
            } else {
                self.metrics
                    .cpus()
                    .iter()
                    .map(|cpu| cpu.cpu_usage())
                    .sum::<f32>()
                    / self.metrics.cpus().len() as f32
            };
            let ram_gib = self.metrics.used_memory() as f64
                / 1024.0
                / 1024.0
                / 1024.0;
            let now = Local::now();
            let controls_left = island.right - 109;
            let island_width = island.right - island.left;

            if island_width >= 690 {
                let cpu_text = wide(&format!("CPU {:>3.0}%", cpu));
                TextOutW(
                    dc,
                    controls_left - 292,
                    island.top + 7,
                    cpu_text.as_ptr(),
                    (cpu_text.len() - 1) as i32,
                );

                let ram_text = wide(&format!("RAM {:.1} GiB", ram_gib));
                TextOutW(
                    dc,
                    controls_left - 202,
                    island.top + 7,
                    ram_text.as_ptr(),
                    (ram_text.len() - 1) as i32,
                );
            }

            if island_width >= 540 {
                let time_text = wide(&now.format("%H:%M").to_string());
                TextOutW(
                    dc,
                    controls_left - 132,
                    island.top + 7,
                    time_text.as_ptr(),
                    (time_text.len() - 1) as i32,
                );

                let date_text = wide(&now.format("%d %b").to_string());
                TextOutW(
                    dc,
                    controls_left - 74,
                    island.top + 7,
                    date_text.as_ptr(),
                    (date_text.len() - 1) as i32,
                );
            }

            for (button_kind, left, right) in [
                (TitleButton::Minimize, island.right - 102, island.right - 68),
                (TitleButton::Maximize, island.right - 68, island.right - 34),
                (TitleButton::Close, island.right - 34, island.right),
            ] {
                if self.hovered_title_button == Some(button_kind) {
                    let hover = CreateSolidBrush(TITLE_HOVER_BG);
                    let hover_rect = RECT {
                        left,
                        top: island.top + 1,
                        right,
                        bottom: island.bottom - 1,
                    };
                    FillRect(dc, &hover_rect, hover);
                    DeleteObject(hover);
                }

                let cx = (left + right) / 2;
                let cy = (island.top + island.bottom) / 2;
                let pressed_offset =
                    if self.pressed_title_button == Some(button_kind) { 1 } else { 0 };
                let glyph_color = if button_kind == TitleButton::Close
                    && self.hovered_title_button == Some(button_kind)
                {
                    TITLE_CLOSE
                } else {
                    TITLE_CONTROL
                };

                let pen = CreatePen(PS_SOLID, 2, glyph_color);
                let old_pen = SelectObject(dc, pen);
                let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));

                match button_kind {
                    TitleButton::Minimize => {
                        MoveToEx(dc, cx - 5, cy - 2 + pressed_offset, null_mut());
                        LineTo(dc, cx, cy + 3 + pressed_offset);
                        LineTo(dc, cx + 5, cy - 2 + pressed_offset);
                    }
                    TitleButton::Maximize => {
                        MoveToEx(dc, cx - 5, cy + 2 + pressed_offset, null_mut());
                        LineTo(dc, cx, cy - 3 + pressed_offset);
                        LineTo(dc, cx + 5, cy + 2 + pressed_offset);
                    }
                    TitleButton::Close => {
                        Ellipse(
                            dc,
                            cx - 6,
                            cy - 5 + pressed_offset,
                            cx + 7,
                            cy + 8 + pressed_offset,
                        );
                        MoveToEx(dc, cx, cy - 7 + pressed_offset, null_mut());
                        LineTo(dc, cx, cy + 1 + pressed_offset);
                    }
                }

                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(pen);
            }

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
            let base_color = if self.appearance.backdrop == "solid" {
                parse_hex_color(&self.appearance.background_color).unwrap_or(BG)
            } else {
                0x000000
            };
            let glass = CreateSolidBrush(base_color);
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

                let _ = state.pty.paste(&text);
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
                apply_window_effects(hwnd, &state.appearance);

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

                let island = title_island_rect(hwnd);
                if x >= island.left
                    && x < island.right
                    && y >= island.top
                    && y < island.bottom
                {
                    return HTCAPTION as isize;
                }

                HTCLIENT as isize
            }

            WM_SIZE => {
                // A minimized window has no usable viewport; retain the editor's
                // last size until Windows supplies its restored dimensions.
                if wp == SIZE_MINIMIZED as usize { return 0; }
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

                if state.status_refreshed.elapsed() >= Duration::from_secs(1) {
                    state.metrics.refresh_all();
                    state.status_refreshed = Instant::now();
                    changed = true;
                }

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

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                // A previous shortcut without WM_CHAR (e.g. Shift+Insert) must
                // not swallow the first character typed after it.
                state.suppress_char = false;
                let alt = GetKeyState(VK_MENU as i32) < 0;
                let altgr = GetKeyState(VK_RMENU as i32) < 0 && GetKeyState(VK_CONTROL as i32) < 0;
                let ctrl = GetKeyState(VK_CONTROL as i32) < 0 && !altgr;
                let shift = GetKeyState(VK_SHIFT as i32) < 0;
                if alt && wp == VK_F4 as usize { return DefWindowProcW(hwnd, msg, wp, lp); }

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
                    if alt && !altgr { modifiers |= CtKeyModifiers::ALT; }
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
                        key if (VK_F1..=VK_F12).contains(&key) => Some(CtKeyCode::F((key - VK_F1 + 1) as u8)),
                        VK_SPACE if ctrl => Some(CtKeyCode::Char(' ')),
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

            WM_CHAR | WM_SYSCHAR => {
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

                if state.pty.raw_mode() {
                    let mut modifiers = CtKeyModifiers::NONE;
                    if msg == WM_SYSCHAR { modifiers |= CtKeyModifiers::ALT; }
                    if GetKeyState(VK_SHIFT as i32) < 0 { modifiers |= CtKeyModifiers::SHIFT; }
                    for ch in text.chars() {
                        let _ = state.pty.send_raw_key(CtKeyEvent::new(CtKeyCode::Char(ch), modifiers));
                    }
                } else { state.input(text.as_bytes()); }
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

                let island = title_island_rect(hwnd);
                if x >= island.left
                    && x < island.right
                    && y >= island.top
                    && y < island.bottom
                {
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
            pty.write(b"echo SST_NATIVE_OK\r")?;
            sent = true;
        }

        if sent && contents.matches("SST_NATIVE_OK").count() >= 2 {
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
