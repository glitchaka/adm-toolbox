mod brush_builtin;

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use brush_builtins::{BuiltinSet, ShellBuilderExt as _};
use brush_core::{
    ExecutionControlFlow, Shell as BrushEngine, SourceInfo,
    builtins,
    extensions::DefaultShellExtensions,
};
use rustyline::{
    Context as RustylineContext, Editor, Helper,
    completion::{Completer, FilenameCompleter, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::{Hinter, HistoryHinter},
    history::DefaultHistory,
    validate::Validator,
};
use tokio::runtime::Runtime;

use brush_builtin::{AdmBuiltin, builtin_names};

struct ShellHelper {
    files: FilenameCompleter,
    hinter: HistoryHinter,
    commands: Vec<String>,
}

impl ShellHelper {
    fn new() -> Self {
        Self {
            files: FilenameCompleter::new(),
            hinter: HistoryHinter::new(),
            commands: completion_commands(),
        }
    }

    fn candidates_for(&self, line: &str, pos: usize) -> Option<(usize, Vec<Pair>)> {
        let before = &line[..pos];
        let segment_start = before
            .char_indices()
            .rev()
            .find(|(_, ch)| *ch == ';' || *ch == '|' || *ch == '&')
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);

        let segment = &before[segment_start..];
        let leading = segment.len() - segment.trim_start().len();
        let content_start = segment_start + leading;
        let trimmed = segment.trim_start();
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let ends_with_space = trimmed.chars().last().is_some_and(char::is_whitespace);

        if words.is_empty() || (words.len() == 1 && !ends_with_space) {
            let typed = words.first().copied().unwrap_or("");
            let mut pairs: Vec<Pair> = self
                .commands
                .iter()
                .filter(|command| command.starts_with(typed))
                .map(|command| Pair {
                    display: command.clone(),
                    replacement: command.clone(),
                })
                .collect();

            pairs.sort_by(|a, b| a.display.cmp(&b.display));
            return Some((content_start, pairs));
        }

        let command = words.first().copied().unwrap_or("");
        let subcommands: &[&str] = match command {
            "net" => &[
                "interfaces",
                "connections",
                "routes",
                "dns",
                "ping",
                "trace",
                "scan",
                "monitor",
                "presence",
                "neighbors",
                "ports",
                "traffic",
                "usage",
                "provider",
            ],
            "sys" => &[
                "info",
                "processes",
                "top",
                "disks",
                "memory",
                "hostname",
                "whoami",
                "uname",
                "kill",
                "services",
            ],
            "device" => &["list", "show", "add", "remove", "path"],
            "domain" => &["status"],
            "switch" => &["list", "show", "add", "remove", "locate", "capabilities", "path"],
            "diag" => &["network", "dns", "hardware", "storage", "traffic", "domain"],
            "config" => &["path", "edit", "reload"],
            _ => &[],
        };

        let currently_second =
            words.len() == 1 && ends_with_space || words.len() == 2 && !ends_with_space;

        if currently_second && !subcommands.is_empty() {
            let typed = if words.len() >= 2 { words[1] } else { "" };
            let start = if words.len() >= 2 {
                pos.saturating_sub(typed.len())
            } else {
                pos
            };

            let pairs = subcommands
                .iter()
                .filter(|candidate| candidate.starts_with(typed))
                .map(|candidate| Pair {
                    display: (*candidate).to_owned(),
                    replacement: (*candidate).to_owned(),
                })
                .collect();

            return Some((start, pairs));
        }

        None
    }
}

impl Completer for ShellHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        context: &RustylineContext<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if let Some(result) = self.candidates_for(line, pos) {
            if !result.1.is_empty() {
                return Ok(result);
            }
        }

        self.files.complete(line, pos, context)
    }
}

impl Hinter for ShellHelper {
    type Hint = String;

    fn hint(
        &self,
        line: &str,
        pos: usize,
        context: &RustylineContext<'_>,
    ) -> Option<Self::Hint> {
        self.hinter.hint(line, pos, context)
    }
}

impl Highlighter for ShellHelper {}
impl Validator for ShellHelper {}
impl Helper for ShellHelper {}

pub struct Shell {
    editor: Editor<ShellHelper, DefaultHistory>,
    engine: BrushEngine<DefaultShellExtensions>,
    runtime: Runtime,
    history_file: PathBuf,
    running: bool,
}

impl Shell {
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("No se pudo inicializar el runtime de la shell")?;

        ensure_portable_config()?;

        let mut engine = runtime.block_on(build_brush_engine())?;
        runtime.block_on(install_adm_bootstrap(&mut engine))?;

        let history_file = home_dir().join(".adm_toolbox_history");
        let mut editor = Editor::<ShellHelper, DefaultHistory>::new()?;
        editor.set_helper(Some(ShellHelper::new()));
        let _ = editor.load_history(&history_file);

        Ok(Self {
            editor,
            engine,
            runtime,
            history_file,
            running: true,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        self.print_banner();

        while self.running {
            let prompt = self.prompt();

            match self.editor.readline(&prompt) {
                Ok(line) => {
                    let line = line.trim_end();

                    if line.trim().is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(line);
                    self.execute(line);
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                }
                Err(ReadlineError::Eof) => {
                    println!();
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }

        let _ = self.editor.save_history(&self.history_file);
        Ok(())
    }

    fn execute(&mut self, line: &str) {
        let history_command = format!("history -s -- {}", bash_quote(line));

        let result = self.runtime.block_on(async {
            let history_params = self.engine.default_exec_params();
            let _ = self
                .engine
                .run_string(
                    &history_command,
                    &SourceInfo::default(),
                    &history_params,
                )
                .await;

            let params = self.engine.default_exec_params();
            self.engine
                .run_string(line, &SourceInfo::default(), &params)
                .await
        });

        match result {
            Ok(result) => {
                if matches!(result.next_control_flow, ExecutionControlFlow::ExitShell) {
                    self.running = false;
                }
            }
            Err(error) => {
                eprintln!("adm: {error}");
            }
        }
    }

    fn prompt(&self) -> String {
        let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
        let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
        let cwd = display_path(self.engine.working_dir());

        format!(
            "\x1b[38;5;42m{user}@{host}\x1b[0m \x1b[38;5;39m{cwd}\x1b[0m\n$ "
        )
    }

    fn print_banner(&self) {
        println!("\x1b[38;5;42mADM Toolbox 0.2.0\x1b[0m");
        println!("Bash-compatible Rust administration shell · help para comenzar");
        println!();
    }
}

async fn build_brush_engine() -> Result<BrushEngine<DefaultShellExtensions>> {
    let registration =
        builtins::simple_builtin::<AdmBuiltin, DefaultShellExtensions>();

    let mut builder = BrushEngine::builder()
        .interactive(true)
        .shell_name("adm-toolbox".to_owned())
        .shell_product_display_str("ADM Toolbox · Rust Bash engine".to_owned())
        .default_builtins(BuiltinSet::BashMode);

    for name in builtin_names() {
        builder = builder.builtin((*name).to_owned(), registration.clone());
    }

    Ok(builder.build().await?)
}

async fn install_adm_bootstrap(
    engine: &mut BrushEngine<DefaultShellExtensions>,
) -> Result<()> {
    let config = portable_config_path();
    let config_text = config.to_string_lossy();
    let quoted_config = bash_quote(&config_text);

    let script = format!(
        r#"
export ADM_CONFIG={quoted_config}

config() {{
    case "$1" in
        reload)
            if [ -f "$ADM_CONFIG" ]; then
                . "$ADM_CONFIG"
            fi
            ;;
        *)
            adm-config "$@"
            ;;
    esac
}}

cd() {{
    case "$1" in
        /[A-Za-z]/*)
            builtin cd "$(adm-path "$1")"
            ;;
        *)
            builtin cd "$@"
            ;;
    esac
}}

if [ -f "$ADM_CONFIG" ]; then
    . "$ADM_CONFIG"
fi
"#
    );

    let params = engine.default_exec_params();
    engine
        .run_string(&script, &SourceInfo::default(), &params)
        .await?;

    Ok(())
}

pub fn resolve_path(cwd: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir();
    }

    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home_dir().join(rest);
    }

    #[cfg(target_os = "windows")]
    if raw.len() >= 3 && raw.starts_with('/') && raw.as_bytes()[2] == b'/' {
        let drive = raw.chars().nth(1).unwrap_or('c').to_ascii_uppercase();
        let rest = &raw[3..];
        return PathBuf::from(format!("{drive}:\\")).join(rest.replace('/', "\\"));
    }

    let path = PathBuf::from(raw);

    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn ensure_portable_config() -> Result<()> {
    let path = portable_config_path();

    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let default_config = r#"# ADM Toolbox portable shell configuration
# Bash-compatible syntax.

alias ll='ls -la'
alias la='ls -a'
alias cls='clear'

# Ejemplos:
# export ADM_SITE='laboratorio'
# alias scanlab='net scan 192.168.1.0/24'
"#;

    fs::write(&path, default_config)
        .with_context(|| format!("No se pudo crear {}", path.display()))?;

    Ok(())
}

pub(crate) fn portable_config_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("config")
        .join("admrc")
}

fn home_dir() -> PathBuf {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

fn display_path(path: &Path) -> String {
    let home = home_dir();

    #[cfg(target_os = "windows")]
    {
        let path_text = path.to_string_lossy().replace('\\', "/");
        let home_text = home.to_string_lossy().replace('\\', "/");
        let path_lower = path_text.to_ascii_lowercase();
        let home_lower = home_text.to_ascii_lowercase();

        if path_lower == home_lower {
            return "~".to_owned();
        }

        if path_lower.starts_with(&(home_lower.clone() + "/")) {
            let relative = &path_text[home_text.len() + 1..];
            return format!("~/{relative}");
        }

        let bytes = path_text.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let rest = &path_text[3..];

            if rest.is_empty() {
                return format!("/{drive}");
            }

            return format!("/{drive}/{rest}");
        }

        path_text
    }

    #[cfg(not(target_os = "windows"))]
    {
        if path == home {
            "~".to_owned()
        } else if let Ok(relative) = path.strip_prefix(&home) {
            format!("~/{}", relative.display())
        } else {
            path.to_string_lossy().into_owned()
        }
    }
}

fn bash_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn completion_commands() -> Vec<String> {
    let builtins = [
        "help",
        "man",
        "cd",
        "pwd",
        "clear",
        "history",
        "alias",
        "unalias",
        "export",
        "env",
        "source",
        "config",
        "exit",
        "logout",
        "vim",
        "edit",
        "ls",
        "cat",
        "head",
        "tail",
        "grep",
        "wc",
        "sort",
        "uniq",
        "cut",
        "tee",
        "less",
        "more",
        "sed",
        "awk",
        "diff",
        "sha256sum",
        "base64",
        "find",
        "printf",
        "basename",
        "dirname",
        "realpath",
        "date",
        "sleep",
        "true",
        "false",
        "touch",
        "mkdir",
        "rm",
        "cp",
        "mv",
        "which",
        "type",
        "ps",
        "top",
        "df",
        "free",
        "hostname",
        "whoami",
        "uname",
        "kill",
        "sys",
        "net",
        "domain",
        "device",
        "switch",
        "wol",
        "diag",
        "jobs",
        "fg",
        "bg",
        "set",
        "shopt",
        "read",
        "mapfile",
        "declare",
        "local",
        "return",
        "break",
        "continue",
    ];

    let mut commands: BTreeSet<String> =
        builtins.iter().map(|value| (*value).to_owned()).collect();

    if let Some(path) = env::var_os("PATH") {
        let extensions: Vec<String> = env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_owned())
            .split(';')
            .map(|value| value.to_ascii_lowercase())
            .collect();

        for directory in env::split_paths(&path) {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();

                if !path.is_file() {
                    continue;
                }

                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(|value| format!(".{}", value.to_ascii_lowercase()));

                if extension
                    .as_ref()
                    .is_some_and(|extension| extensions.contains(extension))
                {
                    if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                        commands.insert(stem.to_owned());
                    }
                }
            }
        }
    }

    commands.into_iter().collect()
}
