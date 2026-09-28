use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;
use xilem::Color;

use crate::adapters::persistence::AppPaths;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(super) struct TerminalAppearance {
    pub(super) backdrop: String,
    pub(super) background_opacity: u8,
    pub(super) background_color: String,
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

impl TerminalAppearance {
    pub(super) fn tint(&self, focused: bool) -> Color {
        let (r, g, b) = parse_rgb(&self.background_color).unwrap_or((0x11, 0x16, 0x29));
        if self.backdrop == "solid" {
            return Color::from_rgb8(r, g, b);
        }

        // SST intentionally drops the terminal tint when the window is inactive.
        // This preserves the existing "text floating over what is behind it" behaviour.
        let alpha = if focused {
            ((self.background_opacity as u16 * 255) / 100) as u8
        } else {
            0
        };
        Color::from_rgba8(r, g, b, alpha)
    }
}

pub(super) fn load(paths: &AppPaths) -> Result<TerminalAppearance> {
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

    parse_rgb(&config.appearance.background_color)
        .with_context(|| "terminal.toml: appearance.background_color inválido")?;

    Ok(config.appearance)
}

fn parse_rgb(value: &str) -> Result<(u8, u8, u8)> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        anyhow::bail!("el color debe usar formato #RRGGBB");
    }

    let rgb = u32::from_str_radix(hex, 16)?;
    Ok((
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ))
}
