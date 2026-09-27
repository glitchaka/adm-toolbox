use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use zip::ZipArchive;

use crate::{
    adapters::terminal::{guard::AlternateScreenGuard, io as terminal_io},
    core::ports::TextEditor,
};

pub const HELIX_SST_VERSION: &str = "0.1.0";
pub const HELIX_UPSTREAM_VERSION: &str = "25.07.1";

#[cfg(windows)]
static HELIX_ARCHIVE: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/helix-25.07.1-x86_64-windows.zip"
));

const CONFIG_TOML: &str = r#"theme = "shell-shock"

[editor]
line-number = "absolute"
mouse = false
true-color = true
cursorline = true
bufferline = "multiple"
color-modes = true

[editor.statusline]
left = ["mode", "spinner", "file-name", "file-modification-indicator"]
center = []
right = ["diagnostics", "selections", "position", "file-encoding", "file-type"]

[editor.statusline.mode]
normal = "SST NOR"
insert = "SST INS"
select = "SST SEL"
"#;

const THEME_TOML: &str = r##"inherits = "base16_default_dark"

"ui.background" = { bg = "#111629" }
"ui.text" = "#DFE8EF"
"ui.text.focus" = { fg = "#FFFFFF", modifiers = ["bold"] }
"ui.cursor" = { fg = "#111629", bg = "#69C7FF" }
"ui.cursor.primary" = { fg = "#111629", bg = "#69C7FF" }
"ui.cursorline.primary" = { bg = "#171D31" }
"ui.selection" = { bg = "#293B59" }
"ui.statusline" = { fg = "#DFE8EF", bg = "#0A0D14" }
"ui.statusline.normal" = { fg = "#111629", bg = "#69C7FF", modifiers = ["bold"] }
"ui.statusline.insert" = { fg = "#111629", bg = "#F6C61B", modifiers = ["bold"] }
"ui.statusline.select" = { fg = "#111629", bg = "#FF9FBD", modifiers = ["bold"] }
"ui.bufferline" = { fg = "#AFA39D", bg = "#0A0D14" }
"ui.bufferline.active" = { fg = "#FFFFFF", bg = "#293B59", modifiers = ["bold"] }
"ui.linenr" = "#756E67"
"ui.linenr.selected" = { fg = "#69C7FF", modifiers = ["bold"] }
"diagnostic.error" = { underline = { color = "#FF2D38", style = "curl" } }
"diagnostic.warning" = { underline = { color = "#F6C61B", style = "curl" } }
"diagnostic.info" = { underline = { color = "#69C7FF", style = "curl" } }
"diagnostic.hint" = { underline = { color = "#FF9FBD", style = "curl" } }
"##;

const NOTICE: &str = r#"helix-sst 0.1.0

This integration bundles Helix 25.07.1.
Upstream project: https://github.com/helix-editor/helix
License: Mozilla Public License 2.0 (MPL-2.0)

Helix is developed by the Helix contributors.
Shell Shock Tool provides the portable packaging, configuration, theme,
terminal bridge, command integration and helix-sst branding.
"#;

pub struct HelixSstEditor;

struct Install {
    hx: PathBuf,
    runtime: PathBuf,
    config: PathBuf,
}

impl TextEditor for HelixSstEditor {
    fn edit(&self, args: &[String], cwd: &Path) -> Result<()> {
        #[cfg(windows)]
        {
            let install = ensure_installed()?;
            run_helix(&install, args, cwd)
        }

        #[cfg(not(windows))]
        {
            let _ = (args, cwd);
            anyhow::bail!("helix-sst está integrado actualmente para Windows")
        }
    }
}

#[cfg(windows)]
fn ensure_installed() -> Result<Install> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .context("No se pudo determinar el directorio de Shell Shock Tool")?
        .to_path_buf();

    let root = exe_dir
        .join("data")
        .join("helix-sst")
        .join(HELIX_UPSTREAM_VERSION);
    let config_dir = exe_dir.join("config").join("helix-sst");
    let marker = root.join(".installed");

    if !marker.is_file() {
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;

        let cursor = Cursor::new(HELIX_ARCHIVE);
        let mut archive = ZipArchive::new(cursor)
            .context("El paquete embebido de Helix no es un ZIP válido")?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
                continue;
            };
            let destination = root.join(relative);

            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
                continue;
            }

            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut output = fs::File::create(&destination)?;
            std::io::copy(&mut entry, &mut output)?;
        }

        fs::write(&marker, format!("helix-upstream={HELIX_UPSTREAM_VERSION}\n"))?;
    }

    let hx = find_named(&root, "hx.exe", false)
        .context("El paquete de Helix no contiene hx.exe")?;
    let runtime = find_named(&root, "runtime", true)
        .context("El paquete de Helix no contiene el directorio runtime")?;

    fs::create_dir_all(&config_dir)?;
    let config = config_dir.join("config.toml");
    fs::write(&config, CONFIG_TOML)?;

    let themes = runtime.join("themes");
    fs::create_dir_all(&themes)?;
    fs::write(themes.join("shell-shock.toml"), THEME_TOML)?;
    fs::write(root.join("HELIX-SST-NOTICE.txt"), NOTICE)?;

    Ok(Install { hx, runtime, config })
}

#[cfg(windows)]
fn find_named(root: &Path, name: &str, directory: bool) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let matches_kind = if directory { path.is_dir() } else { path.is_file() };
        if matches_kind
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_named(&path, name, directory) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(windows)]
fn run_helix(install: &Install, args: &[String], cwd: &Path) -> Result<()> {
    let _guard = AlternateScreenGuard::enter()?;
    let (cols, rows) = terminal_io::size()?;
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut command = CommandBuilder::new(&install.hx);
    command.cwd(cwd);
    command.env("HELIX_RUNTIME", install.runtime.to_string_lossy().as_ref());
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.arg("--config");
    command.arg(&install.config);
    for arg in args {
        command.arg(arg);
    }

    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>();

    thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if output_tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        while let Ok(bytes) = output_rx.try_recv() {
            terminal_io::write(&bytes)?;
        }

        if child.try_wait()?.is_some() {
            while let Ok(bytes) = output_rx.recv_timeout(Duration::from_millis(15)) {
                terminal_io::write(&bytes)?;
            }
            break;
        }

        if terminal_io::poll(Duration::from_millis(15))? {
            match terminal_io::read()? {
                Event::Key(key) => {
                    if let Some(bytes) = encode_key(key) {
                        writer.write_all(&bytes)?;
                        writer.flush()?;
                    }
                }
                Event::Paste(text) => {
                    writer.write_all(text.as_bytes())?;
                    writer.flush()?;
                }
                Event::Resize(new_cols, new_rows) => {
                    pair.master.resize(PtySize {
                        rows: new_rows,
                        cols: new_cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }

    let modifiers = key.modifiers;
    let alt = modifiers.contains(KeyModifiers::ALT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let shift = modifiers.contains(KeyModifiers::SHIFT);

    let mut bytes = match key.code {
        KeyCode::Char(ch) if ctrl => {
            let lower = ch.to_ascii_lowercase();
            let code = match lower {
                'a'..='z' => (lower as u8) & 0x1f,
                '[' => 0x1b,
                '\\' => 0x1c,
                ']' => 0x1d,
                '^' => 0x1e,
                '_' => 0x1f,
                '?' => 0x7f,
                _ => return None,
            };
            vec![code]
        }
        KeyCode::Char(ch) => ch.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => csi_key('A', shift, alt, ctrl),
        KeyCode::Down => csi_key('B', shift, alt, ctrl),
        KeyCode::Right => csi_key('C', shift, alt, ctrl),
        KeyCode::Left => csi_key('D', shift, alt, ctrl),
        KeyCode::Home => csi_tilde_or_simple("H", "1~", shift, alt, ctrl),
        KeyCode::End => csi_tilde_or_simple("F", "4~", shift, alt, ctrl),
        KeyCode::Insert => csi_tilde("2~", shift, alt, ctrl),
        KeyCode::Delete => csi_tilde("3~", shift, alt, ctrl),
        KeyCode::PageUp => csi_tilde("5~", shift, alt, ctrl),
        KeyCode::PageDown => csi_tilde("6~", shift, alt, ctrl),
        KeyCode::F(number) => function_key(number, shift, alt, ctrl)?,
        _ => return None,
    };

    if alt && matches!(key.code, KeyCode::Char(_)) {
        bytes.insert(0, 0x1b);
    }

    Some(bytes)
}

fn modifier_code(shift: bool, alt: bool, ctrl: bool) -> u8 {
    1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl)
}

fn csi_key(final_char: char, shift: bool, alt: bool, ctrl: bool) -> Vec<u8> {
    let modifier = modifier_code(shift, alt, ctrl);
    if modifier == 1 {
        format!("\x1b[{final_char}").into_bytes()
    } else {
        format!("\x1b[1;{modifier}{final_char}").into_bytes()
    }
}

fn csi_tilde(sequence: &str, shift: bool, alt: bool, ctrl: bool) -> Vec<u8> {
    let modifier = modifier_code(shift, alt, ctrl);
    if modifier == 1 {
        format!("\x1b[{sequence}").into_bytes()
    } else {
        let stem = sequence.trim_end_matches('~');
        format!("\x1b[{stem};{modifier}~").into_bytes()
    }
}

fn csi_tilde_or_simple(
    simple: &str,
    tilde: &str,
    shift: bool,
    alt: bool,
    ctrl: bool,
) -> Vec<u8> {
    if modifier_code(shift, alt, ctrl) == 1 {
        format!("\x1b[{simple}").into_bytes()
    } else {
        csi_tilde(tilde, shift, alt, ctrl)
    }
}

fn function_key(number: u8, shift: bool, alt: bool, ctrl: bool) -> Option<Vec<u8>> {
    let base = match number {
        1 => "11~",
        2 => "12~",
        3 => "13~",
        4 => "14~",
        5 => "15~",
        6 => "17~",
        7 => "18~",
        8 => "19~",
        9 => "20~",
        10 => "21~",
        11 => "23~",
        12 => "24~",
        _ => return None,
    };
    Some(csi_tilde(base, shift, alt, ctrl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_navigation_keys_are_encoded_for_pty() {
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![3])
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(b"\x1b[1;5A".to_vec())
        );
    }
}
