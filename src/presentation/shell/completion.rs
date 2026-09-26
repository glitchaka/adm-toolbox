use std::{
    collections::BTreeSet,
    env,
    fs,
};

use rustyline::{
    Context as RustylineContext, Helper,
    completion::{Completer, FilenameCompleter, Pair},
    highlight::Highlighter,
    hint::{Hinter, HistoryHinter},
    validate::Validator,
};

pub struct ShellHelper {
    files: FilenameCompleter,
    hinter: HistoryHinter,
    commands: Vec<String>,
}

impl ShellHelper {
    pub fn new(adm_commands: impl IntoIterator<Item = String>) -> Self {
        Self {
            files: FilenameCompleter::new(),
            hinter: HistoryHinter::new(),
            commands: completion_commands(adm_commands),
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
            let pairs = self
                .commands
                .iter()
                .filter(|command| command.starts_with(typed))
                .map(|command| Pair {
                    display: command.clone(),
                    replacement: command.clone(),
                })
                .collect();

            return Some((content_start, pairs));
        }

        let command = words.first().copied().unwrap_or("");
        let subcommands = subcommands(command);
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

fn completion_commands(adm_commands: impl IntoIterator<Item = String>) -> Vec<String> {
    let bash = [
        "alias", "bg", "break", "cd", "continue", "declare", "exit", "export", "fg",
        "history", "jobs", "local", "logout", "mapfile", "read", "return", "set", "shopt",
        "source", "unalias",
    ];

    let mut commands: BTreeSet<String> =
        bash.into_iter().map(str::to_owned).collect();

    commands.extend(adm_commands);

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

fn subcommands(command: &str) -> &'static [&'static str] {
    match command {
        "net" => &[
            "interfaces", "connections", "routes", "dns", "ping", "trace", "scan",
            "monitor", "presence", "neighbors", "ports", "traffic", "usage", "provider",
        ],
        "sys" => &[
            "info", "processes", "top", "disks", "memory", "hostname", "whoami",
            "uname", "kill", "services",
        ],
        "device" => &["list", "show", "add", "remove", "path"],
        "domain" => &["status"],
        "switch" => &["list", "show", "add", "remove", "locate", "capabilities", "path"],
        "diag" => &["network", "dns", "hardware", "storage", "traffic", "domain"],
        "config" | "adm-config" => &["path", "edit", "reload"],
        _ => &[],
    }
}
