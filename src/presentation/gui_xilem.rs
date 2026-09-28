//! Xilem frontend scaffold for Shell Shock Tool.
//!
//! This module intentionally keeps the terminal as the full-window surface and
//! composes the top status island as an overlay. It is introduced in parallel
//! with the current Win32 renderer until the VT renderer and input path are
//! fully migrated.

use anyhow::Result;
use chrono::Local;
use sysinfo::System;
use xilem::{
    Color, EventLoop, WidgetView, WindowOptions, Xilem,
    masonry::properties::types::AsUnit,
    style::Style as _,
    view::{FlexSpacer, flex_col, flex_row, label, sized_box, text_button, zstack},
};

const WINDOW_WIDTH: f64 = 1240.0;
const WINDOW_HEIGHT: f64 = 820.0;
const ISLAND_WIDTH: f64 = 560.0;
const ISLAND_HEIGHT: f64 = 30.0;

#[derive(Debug)]
struct XilemState {
    running: bool,
    cpu: f32,
    ram_gib: f64,
    clock: String,
    date: String,
}

impl XilemState {
    fn snapshot() -> Self {
        let mut metrics = System::new_all();
        metrics.refresh_all();

        let cpu = if metrics.cpus().is_empty() {
            0.0
        } else {
            metrics
                .cpus()
                .iter()
                .map(|cpu| cpu.cpu_usage())
                .sum::<f32>()
                / metrics.cpus().len() as f32
        };
        let ram_gib = metrics.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let now = Local::now();

        Self {
            running: true,
            cpu,
            ram_gib,
            clock: now.format("%H:%M").to_string(),
            date: now.format("%d %b").to_string(),
        }
    }
}

impl xilem::AppState for XilemState {
    fn keep_running(&self) -> bool {
        self.running
    }
}

fn view(state: &mut XilemState) -> impl WidgetView<XilemState> + use<> {
    // Full-window terminal surface. This will be replaced by TerminalView, a
    // custom Masonry widget backed by the existing EmbeddedSession + vt100
    // parser. It deliberately occupies the entire window: the island does not
    // consume terminal layout space.
    let terminal_surface = sized_box(label(""))
        .expand()
        .background_color(Color::TRANSPARENT);

    let island = sized_box(
        flex_row((
            label("SST"),
            label(format!("CPU {:.0}%", state.cpu)),
            label(format!("RAM {:.1} GiB", state.ram_gib)),
            label(state.clock.clone()),
            label(state.date.clone()),
            text_button("⌄", |_| {}),
            text_button("⌃", |_| {}),
            text_button("⏻", |state: &mut XilemState| state.running = false),
        ))
        .gap(10.px()),
    )
    .width(ISLAND_WIDTH.px())
    .height(ISLAND_HEIGHT.px())
    .padding((4.0, 10.0))
    .corner_radius(13.0)
    .background_color(Color::from_rgba8(0x0A, 0x0D, 0x14, 0xE8));

    // The second child fills the same window and only positions the island.
    // No titlebar height is reserved and the terminal remains underneath it.
    let island_overlay = sized_box(flex_col((
        FlexSpacer::Fixed(6.px()),
        flex_row((
            FlexSpacer::Flex(1.0),
            island,
            FlexSpacer::Flex(1.0),
        )),
        FlexSpacer::Flex(1.0),
    )))
    .expand();

    zstack((terminal_surface, island_overlay))
}

pub fn run() -> Result<()> {
    let app = Xilem::new_simple(
        XilemState::snapshot(),
        view,
        WindowOptions::new("Shell Shock Tool")
            .with_decorations(false)
            .with_transparent(true)
            .with_initial_inner_size((WINDOW_WIDTH, WINDOW_HEIGHT))
            .on_close(|state: &mut XilemState| state.running = false),
    );

    app.run_in(EventLoop::with_user_event())?;
    Ok(())
}
