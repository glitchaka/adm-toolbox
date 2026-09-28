use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use arboard::Clipboard;
use crossterm::event::{
    KeyCode as CtKeyCode, KeyEvent as CtKeyEvent, KeyModifiers as CtKeyModifiers,
};
use tracing::{Span, trace_span};
use xilem::{
    Color, FontWeight, Pod, ViewCtx, WidgetView,
    core::{MessageCtx, MessageResult, Mut, View, ViewMarker},
    dpi::{LogicalPosition, PhysicalPosition},
    kurbo::{Affine, Axis, Point, Rect, Size},
    masonry::{
        accesskit::{Node, Role},
        core::{
            AccessCtx, BrushIndex, ChildrenIds, EventCtx, FromDynWidget, LayoutCtx, MeasureCtx,
            NewWidget, NoAction, PaintCtx, PointerButton, PointerButtonEvent, PointerEvent,
            PointerScrollEvent, PointerUpdate, PropertiesMut, PropertiesRef, RegisterCtx,
            StyleProperty, TextEvent, Update, UpdateCtx, Widget, WidgetId, WidgetMut, WidgetPod,
            render_text,
        },
        imaging::Painter,
        layout::{AsUnit, LayoutSize, LenReq, Length, SizeDef},
        parley::{
            FontFamilyName, Layout,
            style::{FontFamily, GenericFamily},
        },
    },
};

use crate::adapters::terminal::embedded::EmbeddedSession;

use super::appearance::TerminalAppearance;

const PAD: f64 = 14.0;
const CELL_WIDTH: f64 = 10.0;
const CELL_HEIGHT: f64 = 23.0;
const FONT_SIZE: f32 = 19.0;
const INITIAL_COLS: u16 = 112;
const INITIAL_ROWS: u16 = 31;

const FG: Color = Color::from_rgb8(0xDF, 0xE8, 0xEF);
const BG: Color = Color::from_rgb8(0x11, 0x16, 0x29);
const CURSOR: Color = Color::from_rgb8(0x69, 0xC7, 0xFF);
const SELECTION: Color = Color::from_rgb8(0x37, 0x4B, 0x70);

pub(super) type SharedTerminal = Arc<Mutex<TerminalModel>>;

pub(super) struct TerminalModel {
    session: EmbeddedSession,
    parser: vt100::Parser,
    selection: Option<(usize, usize)>,
    dragging: bool,
    cursor_on: bool,
    blink_ns: u64,
    repaint: bool,
    focused: bool,
    appearance: TerminalAppearance,
}

impl TerminalModel {
    pub(super) fn new(session: EmbeddedSession, appearance: TerminalAppearance) -> Self {
        Self {
            session,
            parser: vt100::Parser::new(INITIAL_ROWS, INITIAL_COLS, 10_000),
            selection: None,
            dragging: false,
            cursor_on: true,
            blink_ns: 0,
            repaint: true,
            focused: true,
            appearance,
        }
    }

    fn input(&mut self, bytes: &[u8]) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink_ns = 0;
        self.repaint = true;
        let _ = self.session.write(bytes);
    }

    fn paste(&mut self, text: &str) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink_ns = 0;
        self.repaint = true;
        let _ = self.session.paste(text);
    }

    fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(a, b)| a != b)
    }

    fn selected_text(&self) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let Some((a, b)) = self.selection else {
            return String::new();
        };

        let (start, end) = (a.min(b), a.max(b));
        let mut lines = Vec::new();

        for row in 0..rows {
            let mut line = String::new();
            let mut selected = false;

            for col in 0..cols {
                let index = row as usize * cols as usize + col as usize;
                if index < start || index > end {
                    continue;
                }

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

            if selected {
                lines.push(line.trim_end().to_owned());
            }
        }

        lines.join("\r\n")
    }

    fn cell_at(&self, point: Point) -> usize {
        let (rows, cols) = self.parser.screen().size();
        let col = (((point.x - PAD).max(0.0) / CELL_WIDTH).floor() as i32)
            .clamp(0, cols as i32 - 1);
        let row = (((point.y - PAD).max(0.0) / CELL_HEIGHT).floor() as i32)
            .clamp(0, rows as i32 - 1);
        (row * cols as i32 + col) as usize
    }

    fn resize(&mut self, size: Size) {
        let cols = (((size.width - PAD * 2.0) / CELL_WIDTH).floor() as i32)
            .clamp(2, 500) as u16;
        let rows = (((size.height - PAD * 2.0) / CELL_HEIGHT).floor() as i32)
            .clamp(2, 200) as u16;

        if self.parser.screen().size() != (rows, cols) {
            self.parser.screen_mut().set_size(rows, cols);
            self.session.resize(cols, rows);
            self.selection = None;
            self.repaint = true;
        }
    }

    fn tick(&mut self, interval: u64) -> bool {
        let mut changed = false;

        while let Ok(data) = self.session.output.try_recv() {
            self.parser.process(&data);
            changed = true;
        }

        self.blink_ns = self.blink_ns.saturating_add(interval);
        if self.blink_ns >= 550_000_000 {
            self.blink_ns %= 550_000_000;
            self.cursor_on = !self.cursor_on;
            changed = true;
        }

        self.session.check_timeout();
        self.repaint |= changed;
        changed
    }
}

pub(super) struct TerminalView {
    model: SharedTerminal,
}

impl TerminalView {
    pub(super) fn new(model: SharedTerminal) -> Self {
        Self { model }
    }
}

impl ViewMarker for TerminalView {}

impl<State: 'static, Action: 'static> View<State, Action, ViewCtx> for TerminalView {
    type Element = Pod<TerminalWidget>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _state: &mut State) -> (Self::Element, Self::ViewState) {
        (ctx.create_pod(TerminalWidget::new(self.model.clone())), ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        _element: Mut<'_, Self::Element>,
        _state: &mut State,
    ) {
    }

    fn teardown(
        &self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        _element: Mut<'_, Self::Element>,
    ) {
    }

    fn message(
        &self,
        (): &mut Self::ViewState,
        _message: &mut MessageCtx,
        _element: Mut<'_, Self::Element>,
        _state: &mut State,
    ) -> MessageResult<Action> {
        MessageResult::Stale
    }
}

pub(super) struct TerminalWidget {
    model: SharedTerminal,
    glyphs: HashMap<(String, bool), Layout<BrushIndex>>,
}

impl TerminalWidget {
    fn new(model: SharedTerminal) -> Self {
        Self {
            model,
            glyphs: HashMap::new(),
        }
    }

    fn paint_cell_text(
        &mut self,
        ctx: &mut PaintCtx<'_>,
        painter: &mut Painter<'_>,
        text: &str,
        bold: bool,
        point: Point,
        color: Color,
    ) {
        let key = (text.to_owned(), bold);
        if !self.glyphs.contains_key(&key) {
            let (font_ctx, layout_ctx) = ctx.text_contexts();
            let mut builder = layout_ctx.ranged_builder(font_ctx, text, 1.0, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            builder.push_default(StyleProperty::FontSize(FONT_SIZE));
            builder.push_default(StyleProperty::FontWeight(if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            }));
            let mut layout = builder.build(text);
            layout.break_all_lines(None);
            self.glyphs.insert(key.clone(), layout);
        }

        if let Some(layout) = self.glyphs.get(&key) {
            render_text(
                painter,
                Affine::translate((point.x, point.y)),
                layout,
                &[color.into()],
                true,
            );
        }
    }
}

impl Widget for TerminalWidget {
    type Action = NoAction;

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        let Ok(mut model) = self.model.lock() else {
            return;
        };

        match event {
            PointerEvent::Down(PointerButtonEvent {
                button: None | Some(PointerButton::Primary),
                state,
                ..
            }) => {
                ctx.request_focus();
                ctx.capture_pointer();
                let index = model.cell_at(ctx.local_position(state.position));
                model.selection = Some((index, index));
                model.dragging = true;
                model.repaint = true;
                ctx.request_paint_only();
                ctx.set_handled();
            }
            PointerEvent::Move(PointerUpdate { current, .. }) if model.dragging => {
                let index = model.cell_at(ctx.local_position(current.position));
                if let Some((start, _)) = model.selection {
                    model.selection = Some((start, index));
                    model.repaint = true;
                    ctx.request_paint_only();
                }
                ctx.set_handled();
            }
            PointerEvent::Up(PointerButtonEvent {
                button: Some(PointerButton::Primary),
                ..
            })
            | PointerEvent::Cancel(_) => {
                model.dragging = false;
            }
            PointerEvent::Up(PointerButtonEvent {
                button: Some(PointerButton::Secondary),
                ..
            }) => {
                if let Ok(mut clipboard) = Clipboard::new()
                    && let Ok(text) = clipboard.get_text()
                {
                    model.paste(&text);
                    ctx.request_paint_only();
                }
                ctx.set_handled();
            }
            PointerEvent::Scroll(PointerScrollEvent { delta, .. }) => {
                let scale = ctx.scale_factor();
                let line = PhysicalPosition {
                    x: 120.0 * scale,
                    y: 120.0 * scale,
                };
                let page = PhysicalPosition {
                    x: ctx.content_box().width() * scale,
                    y: ctx.content_box().height() * scale,
                };
                let delta = delta.to_pixel_delta(line, page);
                let LogicalPosition { y, .. } = delta.to_logical::<f64>(scale);
                let rows = (y / 40.0).round() as i32;
                if rows != 0 {
                    let current = model.parser.screen().scrollback() as i32;
                    model
                        .parser
                        .screen_mut()
                        .set_scrollback((current + rows).max(0) as usize);
                    model.selection = None;
                    model.repaint = true;
                    ctx.request_paint_only();
                }
                ctx.set_handled();
            }
            _ => {}
        }
    }

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        if route_text_event(&self.model, ctx, event) {
            ctx.set_handled();
        }
    }

    fn on_anim_frame(
        &mut self,
        ctx: &mut UpdateCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        interval: u64,
    ) {
        let Ok(mut model) = self.model.lock() else {
            ctx.request_anim_frame();
            return;
        };

        model.tick(interval);
        if model.repaint {
            model.repaint = false;
            ctx.request_paint_only();
        }
        if model.session.exited() {
            ctx.exit();
            return;
        }
        ctx.request_anim_frame();
    }

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn update(
        &mut self,
        ctx: &mut UpdateCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &Update,
    ) {
        match event {
            Update::WidgetAdded => ctx.request_anim_frame(),
            Update::FontsChanged => {
                self.glyphs.clear();
                ctx.request_paint_only();
            }
            _ => {}
        }
    }

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        len_req: LenReq,
        _cross_length: Option<Length>,
    ) -> Length {
        let min = match axis {
            Axis::Horizontal => 520.0.px(),
            Axis::Vertical => 320.0.px(),
        };
        let max = 16_384.0.px();
        match len_req {
            LenReq::MinContent => min,
            LenReq::MaxContent => max,
            LenReq::FitContent(space) => space.clamp(min, max),
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx<'_>,
        _props: &PropertiesRef<'_>,
        size: Size,
    ) {
        if let Ok(mut model) = self.model.lock() {
            model.resize(size);
        }
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        painter: &mut Painter<'_>,
    ) {
        let Ok(model) = self.model.lock() else {
            return;
        };

        let bounds = ctx.content_box();
        let tint = model.appearance.tint(model.focused);
        if tint.components[3] != 0.0 {
            painter.fill(bounds, tint).draw();
        }

        let screen = model.parser.screen();
        let (rows, cols) = screen.size();
        let cursor = screen.cursor_position();
        let selection = model.selection;
        let cursor_on = model.cursor_on && screen.scrollback() == 0 && !screen.hide_cursor();

        for row in 0..rows {
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }

                let mut fg = terminal_color(cell.fgcolor(), FG);
                let mut bg = terminal_color(cell.bgcolor(), BG);
                let mut paint_background =
                    !matches!(cell.bgcolor(), vt100::Color::Default) || cell.inverse();

                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let index = row as usize * cols as usize + col as usize;
                if selection.is_some_and(|(a, b)| index >= a.min(b) && index <= a.max(b)) {
                    fg = Color::WHITE;
                    bg = SELECTION;
                    paint_background = true;
                }

                if cursor_on && cursor == (row, col) {
                    fg = BG;
                    bg = CURSOR;
                    paint_background = true;
                }

                let x = PAD + col as f64 * CELL_WIDTH;
                let y = PAD + row as f64 * CELL_HEIGHT;
                let width = if cell.is_wide() {
                    CELL_WIDTH * 2.0
                } else {
                    CELL_WIDTH
                };
                let rect = Rect::new(x, y, x + width, y + CELL_HEIGHT);

                if paint_background {
                    painter.fill(rect, bg).draw();
                }

                let text = cell.contents();
                if !text.is_empty() {
                    self.paint_cell_text(
                        ctx,
                        painter,
                        text,
                        cell.bold(),
                        Point::new(x, y),
                        fg,
                    );
                }
            }
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_label("Shell Shock Tool terminal");
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("SstTerminal", id = id.trace())
    }
}

fn route_text_event(model: &SharedTerminal, ctx: &mut EventCtx<'_>, event: &TextEvent) -> bool {
    let Ok(mut model) = model.lock() else {
        return false;
    };

    match event {
        TextEvent::WindowFocusChange(focused) => {
            model.focused = *focused;
            model.repaint = true;
            true
        }
        TextEvent::ClipboardPaste(text) => {
            model.paste(text);
            true
        }
        TextEvent::Ime(xilem::masonry::core::Ime::Commit(text)) => {
            model.input(text.as_bytes());
            true
        }
        TextEvent::Keyboard(key) if key.state.is_down() => {
            handle_key(&mut model, ctx, key)
        }
        _ => false,
    }
}

fn handle_key(
    model: &mut TerminalModel,
    ctx: &mut EventCtx<'_>,
    key: &xilem::masonry::core::keyboard::KeyboardEvent,
) -> bool {
    use xilem::masonry::core::keyboard::{Key, NamedKey};

    let ctrl = key.modifiers.ctrl();
    let shift = key.modifiers.shift();
    let alt = key.modifiers.alt();

    if let Key::Character(text) = &key.key {
        if ctrl && text.as_str().eq_ignore_ascii_case("q") {
            model.session.force_abort();
            model.repaint = true;
            return true;
        }

        if ctrl && text.as_str().eq_ignore_ascii_case("c") && model.has_selection() {
            ctx.set_clipboard(model.selected_text());
            return true;
        }

        if ctrl && shift && text.as_str().eq_ignore_ascii_case("c") {
            let selected = model.selected_text();
            if !selected.is_empty() {
                ctx.set_clipboard(selected);
            }
            return true;
        }
    }

    if matches!(key.key, Key::Named(NamedKey::Insert)) && ctrl && model.has_selection() {
        ctx.set_clipboard(model.selected_text());
        return true;
    }

    if matches!(key.key, Key::Named(NamedKey::Insert)) && shift {
        if let Ok(mut clipboard) = Clipboard::new()
            && let Ok(text) = clipboard.get_text()
        {
            model.paste(&text);
        }
        return true;
    }

    if model.session.raw_mode() {
        let altgr_like = ctrl && alt && matches!(key.key, Key::Character(_));
        let mut modifiers = CtKeyModifiers::NONE;
        if alt && !altgr_like {
            modifiers |= CtKeyModifiers::ALT;
        }
        if ctrl && !altgr_like {
            modifiers |= CtKeyModifiers::CONTROL;
        }
        if shift {
            modifiers |= CtKeyModifiers::SHIFT;
        }

        if let Some(code) = raw_key_code(&key.key, shift) {
            let _ = model.session.send_raw_key(CtKeyEvent::new(code, modifiers));
            model.parser.screen_mut().set_scrollback(0);
            model.selection = None;
            model.repaint = true;
            return true;
        }
        return false;
    }

    match &key.key {
        Key::Character(text) => {
            let altgr_like = ctrl && alt;
            if ctrl && !altgr_like {
                if let Some(ch) = text.chars().next()
                    && text.chars().count() == 1
                    && ch.is_ascii_alphabetic()
                {
                    model.input(&[(ch.to_ascii_uppercase() as u8) & 0x1f]);
                    return true;
                }
                if text.as_str() == " " {
                    model.input(&[0]);
                    return true;
                }
            }

            if alt && !altgr_like {
                model.input(b"\x1b");
            }
            model.input(text.as_bytes());
            true
        }
        Key::Named(NamedKey::Enter) => {
            model.input(b"\r");
            true
        }
        Key::Named(NamedKey::Backspace) => {
            model.input(b"\x7f");
            true
        }
        Key::Named(NamedKey::Tab) => {
            model.input(if shift { b"\x1b[Z" } else { b"\t" });
            true
        }
        Key::Named(NamedKey::Escape) => {
            model.input(b"\x1b");
            true
        }
        Key::Named(NamedKey::ArrowUp) => {
            let seq = if model.parser.screen().application_cursor() {
                b"\x1bOA".as_slice()
            } else {
                b"\x1b[A".as_slice()
            };
            model.input(seq);
            true
        }
        Key::Named(NamedKey::ArrowDown) => {
            let seq = if model.parser.screen().application_cursor() {
                b"\x1bOB".as_slice()
            } else {
                b"\x1b[B".as_slice()
            };
            model.input(seq);
            true
        }
        Key::Named(NamedKey::ArrowRight) => {
            model.input(b"\x1b[C");
            true
        }
        Key::Named(NamedKey::ArrowLeft) => {
            model.input(b"\x1b[D");
            true
        }
        Key::Named(NamedKey::Home) => {
            model.input(b"\x1b[H");
            true
        }
        Key::Named(NamedKey::End) => {
            model.input(b"\x1b[F");
            true
        }
        Key::Named(NamedKey::Delete) => {
            model.input(b"\x1b[3~");
            true
        }
        Key::Named(NamedKey::Insert) => {
            model.input(b"\x1b[2~");
            true
        }
        Key::Named(NamedKey::PageUp) => {
            model.input(b"\x1b[5~");
            true
        }
        Key::Named(NamedKey::PageDown) => {
            model.input(b"\x1b[6~");
            true
        }
        _ => false,
    }
}

fn raw_key_code(
    key: &xilem::masonry::core::keyboard::Key,
    shift: bool,
) -> Option<CtKeyCode> {
    use xilem::masonry::core::keyboard::{Key, NamedKey};

    match key {
        Key::Character(text) => {
            let mut chars = text.chars();
            let ch = chars.next()?;
            (chars.next().is_none()).then_some(CtKeyCode::Char(ch))
        }
        Key::Named(NamedKey::Escape) => Some(CtKeyCode::Esc),
        Key::Named(NamedKey::Enter) => Some(CtKeyCode::Enter),
        Key::Named(NamedKey::Tab) if shift => Some(CtKeyCode::BackTab),
        Key::Named(NamedKey::Tab) => Some(CtKeyCode::Tab),
        Key::Named(NamedKey::Backspace) => Some(CtKeyCode::Backspace),
        Key::Named(NamedKey::ArrowUp) => Some(CtKeyCode::Up),
        Key::Named(NamedKey::ArrowDown) => Some(CtKeyCode::Down),
        Key::Named(NamedKey::ArrowLeft) => Some(CtKeyCode::Left),
        Key::Named(NamedKey::ArrowRight) => Some(CtKeyCode::Right),
        Key::Named(NamedKey::Home) => Some(CtKeyCode::Home),
        Key::Named(NamedKey::End) => Some(CtKeyCode::End),
        Key::Named(NamedKey::Delete) => Some(CtKeyCode::Delete),
        Key::Named(NamedKey::Insert) => Some(CtKeyCode::Insert),
        Key::Named(NamedKey::PageUp) => Some(CtKeyCode::PageUp),
        Key::Named(NamedKey::PageDown) => Some(CtKeyCode::PageDown),
        Key::Named(NamedKey::F1) => Some(CtKeyCode::F(1)),
        Key::Named(NamedKey::F2) => Some(CtKeyCode::F(2)),
        Key::Named(NamedKey::F3) => Some(CtKeyCode::F(3)),
        Key::Named(NamedKey::F4) => Some(CtKeyCode::F(4)),
        Key::Named(NamedKey::F5) => Some(CtKeyCode::F(5)),
        Key::Named(NamedKey::F6) => Some(CtKeyCode::F(6)),
        Key::Named(NamedKey::F7) => Some(CtKeyCode::F(7)),
        Key::Named(NamedKey::F8) => Some(CtKeyCode::F(8)),
        Key::Named(NamedKey::F9) => Some(CtKeyCode::F(9)),
        Key::Named(NamedKey::F10) => Some(CtKeyCode::F(10)),
        Key::Named(NamedKey::F11) => Some(CtKeyCode::F(11)),
        Key::Named(NamedKey::F12) => Some(CtKeyCode::F(12)),
        _ => None,
    }
}

fn terminal_color(value: vt100::Color, default: Color) -> Color {
    const COLORS: [(u8, u8, u8); 16] = [
        (0x11, 0x16, 0x29),
        (0xF2, 0x6B, 0x6B),
        (0xA3, 0xC7, 0x86),
        (0xE8, 0xCC, 0x83),
        (0x82, 0xAD, 0xE0),
        (0xC9, 0x9F, 0xCE),
        (0xC0, 0xCD, 0xD7),
        (0xDF, 0xE8, 0xEF),
        (0x67, 0x6E, 0x75),
        (0xFF, 0x87, 0x87),
        (0xC4, 0xEB, 0xA8),
        (0xFF, 0xE8, 0xA6),
        (0xA8, 0xD1, 0xFF),
        (0xEB, 0xC1, 0xF0),
        (0xE2, 0xEF, 0xF9),
        (0xFF, 0xFF, 0xFF),
    ];

    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
        vt100::Color::Idx(i) if i < 16 => {
            let (r, g, b) = COLORS[i as usize];
            Color::from_rgb8(r, g, b)
        }
        vt100::Color::Idx(i) if i >= 232 => {
            let v = 8 + (i - 232) * 10;
            Color::from_rgb8(v, v, v)
        }
        vt100::Color::Idx(i) => {
            let i = i - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            Color::from_rgb8(component(i / 36), component(i / 6 % 6), component(i % 6))
        }
    }
}

pub(super) fn focus_root<State, Action, V>(
    model: SharedTerminal,
    child: V,
) -> FocusRoot<V, State, Action>
where
    State: 'static,
    Action: 'static,
    V: WidgetView<State, Action>,
{
    FocusRoot {
        model,
        child,
        phantom: PhantomData,
    }
}

pub(super) struct FocusRoot<V, State, Action> {
    model: SharedTerminal,
    child: V,
    phantom: PhantomData<fn() -> (State, Action)>,
}

impl<V, State, Action> ViewMarker for FocusRoot<V, State, Action> {}

impl<State, Action, V> View<State, Action, ViewCtx> for FocusRoot<V, State, Action>
where
    State: 'static,
    Action: 'static,
    V: WidgetView<State, Action>,
{
    type Element = Pod<FocusRootWidget>;
    type ViewState = V::ViewState;

    fn build(&self, ctx: &mut ViewCtx, state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child, child_state) = self.child.build(ctx, state);
        (
            ctx.create_pod(FocusRootWidget::new(
                child.new_widget.erased(),
                self.model.clone(),
            )),
            child_state,
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        state: &mut State,
    ) {
        let mut child = FocusRootWidget::child_mut(&mut element);
        self.child
            .rebuild(&prev.child, view_state, ctx, child.downcast(), state);
    }

    fn teardown(
        &self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
    ) {
        let mut child = FocusRootWidget::child_mut(&mut element);
        self.child.teardown(view_state, ctx, child.downcast());
    }

    fn message(
        &self,
        view_state: &mut Self::ViewState,
        message: &mut MessageCtx,
        mut element: Mut<'_, Self::Element>,
        state: &mut State,
    ) -> MessageResult<Action> {
        let mut child = FocusRootWidget::child_mut(&mut element);
        self.child
            .message(view_state, message, child.downcast(), state)
    }
}

pub(super) struct FocusRootWidget {
    child: WidgetPod<dyn Widget>,
    model: SharedTerminal,
}

impl FocusRootWidget {
    fn new(child: NewWidget<dyn Widget>, model: SharedTerminal) -> Self {
        Self {
            child: child.to_pod(),
            model,
        }
    }

    fn child_mut<'a>(this: &'a mut WidgetMut<'_, Self>) -> WidgetMut<'a, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for FocusRootWidget {
    type Action = NoAction;

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        if ctx.target() == ctx.widget_id() && route_text_event(&self.model, ctx, event) {
            ctx.set_handled();
        }
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        _len_req: LenReq,
        cross_length: Option<Length>,
    ) -> Length {
        ctx.redirect_measurement(&mut self.child, axis, cross_length)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _props: &PropertiesRef<'_>,
        size: Size,
    ) {
        ctx.run_layout(&mut self.child, size);
        ctx.place_child(&mut self.child, Point::ORIGIN);
        ctx.derive_baselines(&self.child);
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("SstFocusRoot", id = id.trace())
    }
}
