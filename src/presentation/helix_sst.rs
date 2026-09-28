use std::{
    fs,
    collections::VecDeque,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc, atomic::{AtomicU64, Ordering}},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use zip::ZipArchive;

use crate::{
    adapters::terminal::{guard::RawModeGuard, io as terminal_io},
    core::ports::TextEditor,
};

pub const HELIX_SST_VERSION: &str = "0.1.1";
pub const HELIX_UPSTREAM_VERSION: &str = "25.07.1";
pub const HELP: &str = include_str!("../../docs/helix-sst.txt");

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
normal = "NORMAL · i: escribir · F1: ayuda"
insert = "INSERTAR · Esc: comandos"
select = "SELECCIÓN · Esc: normal"
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

const NOTICE: &str = r#"helix-sst 0.1.1

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
    launcher: PathBuf,
}

impl TextEditor for HelixSstEditor {
    fn edit(&self, args: &[String], cwd: &Path) -> Result<i32> {
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

    if !marker.is_file() || find_named(&root, "hx.exe", false).is_none()
        || find_named(&root, "runtime", true).is_none() {
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

    }

    let hx = find_named(&root, "hx.exe", false)
        .context("El paquete de Helix no contiene hx.exe")?;
    let runtime = find_named(&root, "runtime", true)
        .context("El paquete de Helix no contiene el directorio runtime")?;

    fs::create_dir_all(&config_dir)?;
    let config = config_dir.join("config.toml");
    write_default(&config, CONFIG_TOML)?;
    fs::write(config_dir.join("primeros-pasos.txt"), HELP)?;

    let themes = runtime.join("themes");
    fs::create_dir_all(&themes)?;
    write_default(&themes.join("shell-shock.toml"), THEME_TOML)?;
    fs::write(root.join("HELIX-SST-NOTICE.txt"), NOTICE)?;
    fs::write(&marker, format!("helix-upstream={HELIX_UPSTREAM_VERSION}\n"))?;

    Ok(Install { hx, runtime, config, launcher: std::env::current_exe()? })
}

fn write_default(path: &Path, content: &str) -> Result<()> {
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(content.as_bytes())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(error) => return Err(error.into()),
    }
    Ok(())
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

/// Disposable overlay leaves the user's persistent configuration intact.
struct SessionFiles { config: PathBuf, paste: PathBuf }
impl Drop for SessionFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.config);
        let _ = fs::remove_file(&self.paste);
    }
}

#[cfg(windows)]
fn session_files(install: &Install) -> Result<SessionFiles> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let tag = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let files = SessionFiles {
        config: install.config.with_file_name(format!("session-{tag}.toml")),
        paste: install.config.with_file_name(format!("paste-{tag}.txt")),
    };
    let exe = install.launcher.to_string_lossy().into_owned();
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&install.config)?)
        .with_context(|| format!("Configuración inválida: {}", install.config.display()))?;
    let root = config.as_table_mut().context("La configuración de Helix debe ser una tabla")?;
    let editor = root.entry("editor").or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut().context("La sección editor debe ser una tabla")?;
    editor.insert("shell".into(), toml::Value::Array(vec![toml::Value::String(exe.clone()), toml::Value::String("-c".into())]));
    let provider = |operation: &str| {
        let mut command = toml::map::Map::new();
        command.insert("command".into(), toml::Value::String(exe.clone()));
        command.insert("args".into(), toml::Value::Array(vec![
            toml::Value::String("--editor-clipboard".into()), toml::Value::String(operation.into()),
            toml::Value::String(files.paste.to_string_lossy().into_owned()),
        ]));
        toml::Value::Table(command)
    };
    let mut custom = toml::map::Map::new();
    // In Helix 25.07 `yank` gets the provider contents; `paste` sets them.
    custom.insert("yank".into(), provider("get"));
    custom.insert("paste".into(), provider("set"));
    let mut clipboard = toml::map::Map::new();
    clipboard.insert("custom".into(), toml::Value::Table(custom));
    editor.insert("clipboard-provider".into(), toml::Value::Table(clipboard));
    let help_path = install.config.with_file_name("primeros-pasos.txt").to_string_lossy().replace('\\', "/");
    let keys = root.entry("keys").or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut().context("La sección keys debe ser una tabla")?;
    for mode in ["normal", "insert", "select"] {
        let table = keys.entry(mode).or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut().context("El mapa de teclas debe ser una tabla")?;
        table.insert("F1".into(), toml::Value::Array(vec![toml::Value::String("normal_mode".into()), toml::Value::String(format!(":open \"{help_path}\""))]));
        let paste = match mode { "insert" => "@<C-r>+", "select" => "replace_selections_with_clipboard", _ => "paste_clipboard_before" };
        table.insert("F12".into(), toml::Value::String(paste.into()));
    }
    fs::write(&files.config, toml::to_string(&config)?)?;
    Ok(files)
}

#[cfg(windows)]
fn run_helix(install: &Install, args: &[String], cwd: &Path) -> Result<i32> {
    let files = session_files(install)?;
    let _guard = RawModeGuard::enter()?;
    struct RestoreScreen;
    impl Drop for RestoreScreen {
        fn drop(&mut self) {
            let _ = terminal_io::write_raw(b"\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1006l\x1b[0m\x1b[?25h\x1b[?1049l");
        }
    }
    let _screen = RestoreScreen;
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
    command.arg(&files.config);
    command.arg("--log");
    command.arg(install.config.with_file_name("helix.log"));
    for arg in args {
        command.arg(arg);
    }

    let force_abort = terminal_io::force_abort_flag();
    force_abort.store(false, Ordering::SeqCst);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
    let protocol = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(rows, cols, 0, super::pty_protocol::Replies::default())));
    let reply_writer = writer.clone();
    let reply_protocol = protocol.clone();
    let (output_tx, output_rx) = mpsc::channel::<std::io::Result<Vec<u8>>>();

    // Start the reader BEFORE spawning Helix. INHERIT_CURSOR can ask for a
    // cursor report during process creation, and waits on its input pipe.
    let reader_thread = thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Err(error) => { let _ = output_tx.send(Err(error)); break; }
                Ok(count) => {
                    let replies = {
                        let mut parser = reply_protocol.lock().unwrap_or_else(|e| e.into_inner());
                        parser.process(&buffer[..count]);
                        std::mem::take(&mut parser.callbacks_mut().bytes)
                    };
                    if !replies.is_empty() {
                        let mut writer = reply_writer.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(error) = writer.write_all(&replies).and_then(|_| writer.flush()) {
                            let _ = output_tx.send(Err(error)); break;
                        }
                    }
                    if output_tx.send(Ok(buffer[..count].to_vec())).is_err() { break; }
                }
            }
        }
    });

    let spawned = pair.slave.spawn_command(command);
    drop(pair.slave);
    let result = match spawned {
        Ok(mut child) => {
            let mut pending_pastes = VecDeque::new();
            let result = (|| -> Result<i32> { loop {
                if force_abort.swap(false, Ordering::SeqCst) {
                    let _ = child.kill();
                    return Ok(130);
                }
                while let Ok(bytes) = output_rx.try_recv() { terminal_io::write_raw(&bytes?)?; }
                if let Some(status) = child.try_wait()? { return Ok(status.exit_code() as i32); }
                // The helper removes a transfer file when Helix has consumed it.
                // Queue rapid paste operations instead of overwriting pending text.
                if !files.paste.exists() {
                    if let Some(text) = pending_pastes.pop_front() {
                        fs::write(&files.paste, text)?;
                        let win32 = protocol.lock().unwrap_or_else(|e| e.into_inner()).callbacks().win32_input;
                        if let Some(bytes) = encode_input(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE), win32) {
                            let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
                            writer.write_all(&bytes)?;
                            writer.flush()?;
                        }
                    }
                }
                if !terminal_io::poll(Duration::from_millis(15))? { continue; }
                let win32 = protocol.lock().unwrap_or_else(|e| e.into_inner()).callbacks().win32_input;
                match terminal_io::read()? {
                    Event::Key(key) => {
                        if let Some(bytes) = encode_input(key, win32) {
                            let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
                            writer.write_all(&bytes)?;
                            writer.flush()?;
                        }
                    }
                    Event::Paste(text) => {
                        // Let Helix insert one clipboard value, preserving newlines
                        // and indentation instead of synthesizing Enter presses.
                        pending_pastes.push_back(text.replace("\r\n", "\n"));
                    }
                    Event::Resize(new_cols, new_rows) => {
                        protocol.lock().unwrap_or_else(|e| e.into_inner()).screen_mut().set_size(new_rows, new_cols);
                        pair.master.resize(PtySize { rows: new_rows, cols: new_cols, pixel_width: 0, pixel_height: 0 })?;
                    }
                    _ => {},
                }
            } })();
            if result.is_err() { let _ = child.kill(); }
            let _ = child.wait();
            result
        }
        Err(error) => Err(error),
    };
    // ConPTY close can block if nobody drains its output. Keep the reader alive
    // until the master has closed, then collect the final repaint/exit output.
    drop(writer);
    drop(pair.master);
    let _ = reader_thread.join();
    for bytes in output_rx {
        if let Ok(bytes) = bytes { terminal_io::write_raw(&bytes)?; }
    }
    result
}

fn encode_input(key: KeyEvent, win32: bool) -> Option<Vec<u8>> {
    if !win32 { return encode_key(key); }
    if key.kind == KeyEventKind::Release { return None; }
    let mut state = 0u32;
    if key.modifiers.contains(KeyModifiers::SHIFT) { state |= 0x10; }
    if key.modifiers.contains(KeyModifiers::ALT) { state |= 0x02; }
    if key.modifiers.contains(KeyModifiers::CONTROL) { state |= 0x08; }
    let (vk, text): (u16, String) = match key.code {
        KeyCode::Char(ch) => {
            let vk = if ch.is_ascii_alphanumeric() { ch.to_ascii_uppercase() as u16 } else { 0 };
            let character = if key.modifiers.contains(KeyModifiers::CONTROL) {
                match ch.to_ascii_lowercase() {
                    'a'..='z' => char::from((ch.to_ascii_lowercase() as u8) & 0x1f),
                    ' ' | '@' | '2' => '\0', '[' => '\x1b', '\\' => '\x1c', ']' => '\x1d',
                    '^' => '\x1e', '_' => '\x1f', '?' => '\x7f', _ => ch,
                }
            } else { ch };
            (vk, character.to_string())
        }
        KeyCode::Enter => (0x0d, "\r".into()), KeyCode::Esc => (0x1b, "\x1b".into()),
        KeyCode::Backspace => (0x08, "\x08".into()), KeyCode::Tab => (0x09, "\t".into()),
        KeyCode::BackTab => { state |= 0x10; (0x09, "\t".into()) },
        KeyCode::Left => (0x25, String::new()), KeyCode::Up => (0x26, String::new()),
        KeyCode::Right => (0x27, String::new()), KeyCode::Down => (0x28, String::new()),
        KeyCode::Home => (0x24, String::new()), KeyCode::End => (0x23, String::new()),
        KeyCode::PageUp => (0x21, String::new()), KeyCode::PageDown => (0x22, String::new()),
        KeyCode::Insert => (0x2d, String::new()), KeyCode::Delete => (0x2e, String::new()),
        KeyCode::F(number @ 1..=24) => (0x70 + u16::from(number) - 1, String::new()),
        _ => return None,
    };
    if matches!(vk, 0x21..=0x28 | 0x2d | 0x2e) { state |= 0x100; }
    #[cfg(windows)]
    let scan = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::MapVirtualKeyW(vk.into(), 0) };
    #[cfg(not(windows))]
    let scan = 0;
    let units: Vec<u16> = if text.is_empty() { vec![0] } else { text.encode_utf16().collect() };
    let mut bytes = Vec::new();
    // Keep UTF-16 surrogate halves adjacent. Crossterm 0.28 also decodes
    // surrogate key-up records as characters, so their releases carry no text.
    for down in [1, 0] {
        for &unit in &units {
            let unit = if down == 0 && units.len() > 1 { 0 } else { unit };
            bytes.extend_from_slice(format!("\x1b[{vk};{scan};{unit};{down};{state};1_").as_bytes());
        }
    }
    Some(bytes)
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
                ' ' | '2' | '@' => 0,
                _ => return None,
            }
            ;
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

    #[test]
    fn helix_conpty_queries_are_answered_across_output_chunks() {
        let mut parser = vt100::Parser::new_with_callbacks(36, 120, 0, super::super::pty_protocol::Replies::default());
        parser.process(b"\x1b[4;8H\x1b[");
        parser.process(b"6n\x1b[c\x1b[18t\x1b[?9001h");
        assert_eq!(parser.callbacks().bytes, b"\x1b[4;8R\x1b[?1;2c\x1b[8;36;120t");
        assert!(parser.callbacks().win32_input);
        parser.process(b"\x1b[?9001l");
        assert!(!parser.callbacks().win32_input);
    }

    #[test]
    fn helix_help_is_available_from_native_shell() -> Result<()> {
        let (mut shell, _, _) = crate::composition::build_engine()?;
        for command in ["help helix", "help hx", "help helix-sst", "helix --help"] {
            let result = shell.execute(command)?;
            assert_eq!(result.status, 0, "{command}: {}", result.stderr);
            assert!(result.stdout.contains("PRIMEROS PASOS"), "{command}");
        }
        let result = shell.execute("help")?;
        assert!(result.stdout.contains("help helix"));
        Ok(())
    }

    /// Runs the bundled editor against the same WindowIo channels as the GUI.
    /// Build the application first: its executable provides the clipboard helper.
    #[cfg(windows)]
    #[test]
    #[ignore = "starts the bundled Helix process and writes isolated editor fixtures"]
    fn helix_live_edit_paste_help_resize_and_exit() -> Result<()> {
        use std::{sync::atomic::AtomicBool, time::{Instant, SystemTime, UNIX_EPOCH}};
        use crate::adapters::terminal::io::WindowIo;

        struct EditorSession {
            input: mpsc::Sender<Event>,
            output: mpsc::Receiver<Vec<u8>>,
            finished: mpsc::Receiver<Result<i32>>,
            parser: vt100::Parser,
            abort: Arc<AtomicBool>,
            raw: Arc<AtomicBool>,
            size: Arc<Mutex<(u16, u16)>>,
        }
        impl Drop for EditorSession {
            fn drop(&mut self) { self.abort.store(true, Ordering::SeqCst); }
        }
        impl EditorSession {
            fn key(&self, code: KeyCode) -> Result<()> {
                self.input.send(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))?;
                Ok(())
            }
            fn text(&self, text: &str) -> Result<()> {
                for character in text.chars() { self.key(KeyCode::Char(character))?; }
                Ok(())
            }
            fn command(&self, command: &str) -> Result<()> {
                self.key(KeyCode::Esc)?;
                self.text(command)?;
                self.key(KeyCode::Enter)
            }
            fn until(&mut self, label: &str, ready: impl Fn(&vt100::Screen) -> bool) -> Result<()> {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    while let Ok(bytes) = self.output.try_recv() { self.parser.process(&bytes); }
                    if ready(self.parser.screen()) { println!("PASS: {label}"); return Ok(()); }
                    if Instant::now() >= deadline {
                        anyhow::bail!("{label}: tiempo agotado. Pantalla:\n{}", self.parser.screen().contents());
                    }
                    match self.output.recv_timeout(Duration::from_millis(50)) {
                        Ok(bytes) => self.parser.process(&bytes),
                        Err(mpsc::RecvTimeoutError::Timeout) => {},
                        Err(error) => anyhow::bail!("{label}: {error}. Pantalla:\n{}", self.parser.screen().contents()),
                    }
                }
            }
            fn saved(&mut self, path: &Path, expected: &str) -> Result<()> {
                self.command(":w")?;
                self.until("guardado exacto en disco", |_| fs::read_to_string(path).ok().as_deref() == Some(expected))
            }
        }

        let mut install = ensure_installed()?;
        let debug_dir = std::env::current_exe()?.parent().and_then(Path::parent)
            .context("directorio del binario de pruebas")?.to_path_buf();
        install.launcher = debug_dir.join("sst.exe");
        anyhow::ensure!(install.launcher.is_file(), "Compila sst antes de esta prueba");
        let tag = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let directory = debug_dir.join(format!("helix-live-{tag}"));
        fs::create_dir_all(&directory)?;
        let typed_file = directory.join("teclado.txt");
        let script_file = directory.join("script.sh");
        let args = vec![typed_file.to_string_lossy().into_owned()];
        let (input, events) = mpsc::channel();
        let (display, output) = mpsc::channel();
        let (finish_tx, finished) = mpsc::channel();
        let raw = Arc::new(AtomicBool::new(false));
        let abort = Arc::new(AtomicBool::new(false));
        let size = Arc::new(Mutex::new((120, 36)));
        let io = WindowIo::new(display, events, size.clone(), raw.clone(), Arc::new(AtomicBool::new(false)), abort.clone());
        let cwd = directory.clone();
        let worker = thread::spawn(move || {
            terminal_io::install(io);
            let _ = finish_tx.send(run_helix(&install, &args, &cwd));
        });
        let mut session = EditorSession { input, output, finished, parser: vt100::Parser::new(36, 120, 0), abort, raw, size };
        session.until("apertura de Helix", |screen| screen.contents().contains("teclado.txt"))?;
        session.command(":line-ending lf")?;
        session.key(KeyCode::Char('i'))?;
        session.until("modo insertar", |screen| screen.contents().contains("INSERTAR"))?;
        session.text("Helix: á ñ λ 😀")?;
        session.until("escritura Unicode", |screen| screen.contents().contains("Helix: á ñ λ 😀"))?;
        session.saved(&typed_file, "Helix: á ñ λ 😀\n")?;

        session.key(KeyCode::F(1))?;
        session.until("ayuda con F1", |screen| screen.contents().contains("primeros-pasos.txt"))?;
        session.command(":bc")?;
        session.until("regreso desde ayuda", |screen| screen.contents().contains("teclado.txt") && !screen.contents().contains("primeros-pasos.txt"))?;

        *session.size.lock().unwrap() = (96, 28);
        session.parser.screen_mut().set_size(28, 96);
        session.input.send(Event::Resize(96, 28))?;
        session.until("redimensionado", |screen| screen.contents().contains("teclado.txt") && screen.cursor_position().0 < 28)?;

        session.command(&format!(":open {}", script_file.to_string_lossy().replace('\\', "/")))?;
        session.until("nuevo script", |screen| screen.contents().contains("script.sh"))?;
        session.command(":line-ending lf")?;
        session.key(KeyCode::Char('i'))?;
        session.until("insertar script", |screen| screen.contents().contains("INSERTAR"))?;
        let script = "#!/usr/bin/env bash\necho 'SST_PASTE_OK á ñ λ 😀'";
        session.input.send(Event::Paste(script.replace('\n', "\r\n")))?;
        session.until("pegado multilínea", |screen| screen.contents().contains("SST_PASTE_OK"))?;
        session.saved(&script_file, &format!("{script}\n"))?;

        session.command(":qa")?;
        let status = session.finished.recv_timeout(Duration::from_secs(10))??;
        assert_eq!(status, 0);
        worker.join().expect("hilo de Helix");
        while let Ok(bytes) = session.output.try_recv() { session.parser.process(&bytes); }
        assert!(!session.raw.load(Ordering::SeqCst), "modo raw restaurado");
        assert!(!session.parser.screen().alternate_screen(), "pantalla principal restaurada");
        println!("PASS: salida limpia y restauración del terminal. Archivos: {}", directory.display());
        Ok(())
    }
}
