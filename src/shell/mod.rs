mod parser;

use std::{
    collections::{BTreeSet, HashMap},
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use rustyline::{
    Context as RustylineContext, Editor, Helper,
    completion::{Completer, FilenameCompleter, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::{Hinter, HistoryHinter},
    history::DefaultHistory,
    validate::Validator,
};

use crate::{commands, editor};
use parser::{ChainOp, ParsedCommand, Pipeline};

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
            .find(|(_, ch)| matches!(ch, ';' | '|' | '&'))
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
                "interfaces", "connections", "routes", "dns", "ping", "trace", "scan",
                "monitor", "neighbors", "ports", "traffic", "usage", "provider",
            ],
            "sys" => &[
                "info", "processes", "top", "disks", "memory", "hostname", "whoami",
                "uname", "kill", "services",
            ],
            "device" => &["list", "show", "add", "remove", "path"],
            "domain" => &["status"],
            "switch" => &["capabilities", "locate"],
            "diag" => &["network", "traffic"],
            "config" => &["path", "edit", "reload"],
            _ => &[],
        };

        let currently_second = words.len() == 1 && ends_with_space
            || words.len() == 2 && !ends_with_space;

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
    cwd: PathBuf,
    running: bool,
    last_status: i32,
    aliases: HashMap<String, String>,
    history: Vec<String>,
    history_file: PathBuf,
    config_file: PathBuf,
}

impl Shell {
    pub fn new() -> Result<Self> {
        let cwd = env::current_dir().context("No se pudo obtener el directorio actual")?;
        let history_file = home_dir().join(".adm_toolbox_history");
        let config_file = portable_config_path();
        let mut editor = Editor::<ShellHelper, DefaultHistory>::new()?;
        editor.set_helper(Some(ShellHelper::new()));
        let _ = editor.load_history(&history_file);

        let mut aliases = HashMap::new();
        aliases.insert("ll".to_owned(), "ls -la".to_owned());
        aliases.insert("la".to_owned(), "ls -a".to_owned());

        let mut shell = Self {
            editor,
            cwd,
            running: true,
            last_status: 0,
            aliases,
            history: Vec::new(),
            history_file,
            config_file,
        };

        shell.load_startup_files()?;
        Ok(shell)
    }

    pub fn run(&mut self) -> Result<()> {
        self.print_banner();

        while self.running {
            let prompt = self.prompt();
            match self.editor.readline(&prompt) {
                Ok(line) => {
                    let line = line.trim().to_owned();
                    if line.is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(line.as_str());
                    self.history.push(line.clone());

                    if let Err(error) = self.execute_line(&line) {
                        eprintln!("adm: {error}");
                        self.last_status = 1;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    self.last_status = 130;
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

    fn print_banner(&self) {
        println!("\x1b[38;5;42mADM Toolbox 0.1.0\x1b[0m");
        println!("Rust administration shell · escribe 'help' para ver comandos");
        println!();
    }

    fn prompt(&self) -> String {
        let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
        let host = env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".to_owned());
        let cwd = display_path(&self.cwd);
        let symbol = if is_elevated_hint() { "#" } else { "$" };

        format!(
            "\x1b[38;5;42m{user}@{host}\x1b[0m \x1b[38;5;39m{cwd}\x1b[0m\n{symbol} "
        )
    }

    fn execute_line(&mut self, line: &str) -> Result<()> {
        let parsed = parser::parse(line)?;
        let mut previous_status = self.last_status;

        for segment in parsed.segments {
            let should_run = match segment.gate {
                None | Some(ChainOp::Always) => true,
                Some(ChainOp::And) => previous_status == 0,
                Some(ChainOp::Or) => previous_status != 0,
            };

            if should_run {
                previous_status = self.execute_pipeline(&segment.pipeline)?;
            }
        }

        self.last_status = previous_status;
        Ok(())
    }

    fn execute_pipeline(&mut self, pipeline: &Pipeline) -> Result<i32> {
        let mut input: Option<Vec<u8>> = if let Some(path) = &pipeline.input_redirect {
            let source = resolve_path(&self.cwd, path);
            Some(
                fs::read(&source)
                    .with_context(|| format!("no se pudo leer {}", source.display()))?,
            )
        } else {
            None
        };
        let mut status = 0;
        let mut stderr_acc = Vec::new();

        for (index, parsed_command) in pipeline.commands.iter().enumerate() {
            let command = self.expand_alias(parsed_command)?;
            if command.argv.is_empty() {
                continue;
            }

            let argv: Vec<String> = command
                .argv
                .iter()
                .map(|arg| expand_arg(arg, &self.cwd, self.last_status))
                .collect();

            let name = argv[0].as_str();
            let args = &argv[1..];

            match name {
                "cd" => {
                    status = self.cmd_cd(args)?;
                    input = Some(Vec::new());
                    continue;
                }
                "exit" | "logout" => {
                    self.running = false;
                    return Ok(0);
                }
                "clear" => {
                    print!("\x1b[2J\x1b[H");
                    std::io::stdout().flush()?;
                    input = Some(Vec::new());
                    status = 0;
                    continue;
                }
                "alias" => {
                    let output = self.cmd_alias(args)?;
                    status = output.0;
                    input = Some(output.1.into_bytes());
                    continue;
                }
                "unalias" => {
                    status = self.cmd_unalias(args);
                    input = Some(Vec::new());
                    continue;
                }
                "export" => {
                    status = self.cmd_export(args);
                    input = Some(Vec::new());
                    continue;
                }
                "env" => {
                    let mut rows: Vec<_> = env::vars().collect();
                    rows.sort_by(|a, b| a.0.cmp(&b.0));
                    input = Some(
                        rows.into_iter()
                            .map(|(key, value)| format!("{key}={value}\n"))
                            .collect::<String>()
                            .into_bytes(),
                    );
                    status = 0;
                    continue;
                }
                "history" => {
                    let text = self
                        .history
                        .iter()
                        .enumerate()
                        .map(|(i, line)| format!("{:>5}  {line}\n", i + 1))
                        .collect::<String>();
                    input = Some(text.into_bytes());
                    status = 0;
                    continue;
                }
                "config" => {
                    let output = self.cmd_config(args)?;
                    status = output.0;
                    input = Some(output.1.into_bytes());
                    continue;
                }
                "source" | "." => {
                    status = self.cmd_source(args)?;
                    input = Some(Vec::new());
                    continue;
                }
                "vim" | "edit" => {
                    let Some(path) = args.first() else {
                        eprintln!("{name}: falta el archivo");
                        return Ok(2);
                    };
                    let path = resolve_path(&self.cwd, path);
                    match editor::run(&path) {
                        Ok(()) => status = 0,
                        Err(error) => {
                            eprintln!("{name}: {error}");
                            status = 1;
                        }
                    }
                    input = Some(Vec::new());
                    continue;
                }
                _ => {}
            }

            if commands::is_internal(name) {
                let output = commands::run(name, args, input.as_deref(), &self.cwd)?;
                status = output.status;
                input = Some(output.stdout.into_bytes());
                if !output.stderr.is_empty() {
                    stderr_acc.extend_from_slice(output.stderr.as_bytes());
                    if !output.stderr.ends_with('\n') {
                        stderr_acc.push(b'\n');
                    }
                }
            } else {
                let is_only_command = pipeline.commands.len() == 1
                    && pipeline.redirect.is_none()
                    && pipeline.input_redirect.is_none()
                    && index == 0
                    && input.is_none();

                if is_only_command {
                    status = self.run_external_interactive(name, args)?;
                    input = Some(Vec::new());
                } else {
                    let output = self.run_external_capture(name, args, input.as_deref())?;
                    status = output.status.code().unwrap_or(1);
                    input = Some(output.stdout);
                    stderr_acc.extend_from_slice(&output.stderr);
                }
            }
        }

        if !stderr_acc.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&stderr_acc));
        }

        let stdout = input.unwrap_or_default();
        if let Some(redirection) = &pipeline.redirect {
            let target = resolve_path(&self.cwd, &redirection.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut options = OpenOptions::new();
            options.create(true).write(true);
            if redirection.append {
                options.append(true);
            } else {
                options.truncate(true);
            }

            let mut file = options
                .open(&target)
                .with_context(|| format!("No se pudo abrir {}", target.display()))?;
            file.write_all(&stdout)?;
        } else if !stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&stdout));
            if !stdout.ends_with(b"\n") {
                println!();
            }
        }

        Ok(status)
    }

    fn expand_alias(&self, command: &ParsedCommand) -> Result<ParsedCommand> {
        let Some(first) = command.argv.first() else {
            return Ok(command.clone());
        };

        let Some(alias) = self.aliases.get(first) else {
            return Ok(command.clone());
        };

        let mut argv = shell_words::split(alias)
            .with_context(|| format!("alias inválido: {first}"))?;
        argv.extend(command.argv.iter().skip(1).cloned());
        Ok(ParsedCommand { argv })
    }

    fn run_external_interactive(&self, name: &str, args: &[String]) -> Result<i32> {
        let status = Command::new(name)
            .args(args)
            .current_dir(&self.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("comando no encontrado: {name}"))?;

        Ok(status.code().unwrap_or(1))
    }

    fn run_external_capture(
        &self,
        name: &str,
        args: &[String],
        input: Option<&[u8]>,
    ) -> Result<std::process::Output> {
        let mut child = Command::new(name)
            .args(args)
            .current_dir(&self.cwd)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("comando no encontrado: {name}"))?;

        if let (Some(bytes), Some(stdin)) = (input, child.stdin.as_mut()) {
            stdin.write_all(bytes)?;
        }

        Ok(child.wait_with_output()?)
    }

    fn load_startup_files(&mut self) -> Result<()> {
        let portable = self.config_file.clone();
        if portable.is_file() {
            let portable_text = portable.to_string_lossy().into_owned();
            let _ = self.cmd_source(&[portable_text])?;
        }

        let home_rc = home_dir().join(".admrc");
        if home_rc.is_file() && home_rc != portable {
            let home_text = home_rc.to_string_lossy().into_owned();
            let _ = self.cmd_source(&[home_text])?;
        }

        Ok(())
    }

    fn cmd_config(&mut self, args: &[String]) -> Result<(i32, String)> {
        match args.first().map(String::as_str).unwrap_or("path") {
            "path" => Ok((
                0,
                format!("{}\n", display_path(&self.config_file)),
            )),
            "edit" => {
                if let Some(parent) = self.config_file.parent() {
                    fs::create_dir_all(parent)?;
                }
                editor::run(&self.config_file)?;
                Ok((0, String::new()))
            }
            "reload" => {
                self.load_startup_files()?;
                Ok((0, "configuración recargada\n".to_owned()))
            }
            other => Ok((
                2,
                format!("config: subcomando desconocido: {other}\n"),
            )),
        }
    }

    fn cmd_cd(&mut self, args: &[String]) -> Result<i32> {
        let target = if let Some(path) = args.first() {
            resolve_path(&self.cwd, path)
        } else {
            home_dir()
        };

        if !target.is_dir() {
            eprintln!("cd: no existe el directorio: {}", target.display());
            return Ok(1);
        }

        self.cwd = target.canonicalize().unwrap_or(target);
        env::set_current_dir(&self.cwd)?;
        Ok(0)
    }

    fn cmd_alias(&mut self, args: &[String]) -> Result<(i32, String)> {
        if args.is_empty() {
            let mut rows: Vec<_> = self.aliases.iter().collect();
            rows.sort_by(|a, b| a.0.cmp(b.0));
            let text = rows
                .into_iter()
                .map(|(name, value)| format!("alias {name}='{value}'\n"))
                .collect();
            return Ok((0, text));
        }

        for arg in args {
            let Some((name, value)) = arg.split_once('=') else {
                if let Some(value) = self.aliases.get(arg) {
                    return Ok((0, format!("alias {arg}='{value}'\n")));
                }
                eprintln!("alias: {arg}: no encontrado");
                return Ok((1, String::new()));
            };
            self.aliases.insert(name.to_owned(), value.to_owned());
        }

        Ok((0, String::new()))
    }

    fn cmd_unalias(&mut self, args: &[String]) -> i32 {
        let mut status = 0;
        for name in args {
            if self.aliases.remove(name).is_none() {
                eprintln!("unalias: {name}: no encontrado");
                status = 1;
            }
        }
        status
    }

    fn cmd_export(&mut self, args: &[String]) -> i32 {
        let mut status = 0;
        for assignment in args {
            let Some((name, value)) = assignment.split_once('=') else {
                eprintln!("export: uso: export NOMBRE=VALOR");
                status = 2;
                continue;
            };
            unsafe {
                env::set_var(name, value);
            }
        }
        status
    }

    fn cmd_source(&mut self, args: &[String]) -> Result<i32> {
        let Some(file) = args.first() else {
            eprintln!("source: falta archivo");
            return Ok(2);
        };

        let path = resolve_path(&self.cwd, file);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("source: no se pudo leer {}", path.display()))?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            self.execute_line(line)?;
            if !self.running {
                break;
            }
        }
        Ok(self.last_status)
    }
}

fn completion_commands() -> Vec<String> {
    let builtins = [
        "help", "man", "cd", "pwd", "clear", "history", "alias", "unalias", "export",
        "env", "source", "config", "exit", "logout", "vim", "edit", "ls", "cat", "head",
        "tail", "grep", "wc", "sort", "uniq", "cut", "tee", "less", "more", "sed", "awk",
        "diff", "sha256sum", "base64", "find", "printf", "basename", "dirname", "realpath",
        "date", "sleep", "true", "false", "touch", "mkdir", "rm", "cp", "mv", "which",
        "type", "ps", "top", "df", "free", "hostname", "whoami", "uname", "kill", "sys",
        "net", "domain", "device", "switch", "wol", "diag",
    ];

    let mut commands: BTreeSet<String> = builtins.iter().map(|value| (*value).to_owned()).collect();

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

fn portable_config_path() -> PathBuf {
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

fn expand_arg(raw: &str, cwd: &Path, last_status: i32) -> String {
    if raw == "$?" {
        return last_status.to_string();
    }
    if raw == "$PWD" {
        return cwd.to_string_lossy().into_owned();
    }

    let mut value = raw.to_owned();
    if value.starts_with('~') {
        value = resolve_path(cwd, &value).to_string_lossy().into_owned();
    }

    let mut output = String::new();
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '$' {
            index += 1;
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }

            if start == index {
                output.push('$');
                continue;
            }

            let name: String = chars[start..index].iter().collect();
            output.push_str(&env::var(name).unwrap_or_default());
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }

    output
}

fn display_path(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        let text = path.to_string_lossy();
        let bytes = text.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            let drive = (bytes[0] as char).to_ascii_lowercase();
            let rest = text[3..].replace('\\', "/");
            if rest.is_empty() {
                return format!("/{drive}");
            }
            return format!("/{drive}/{rest}");
        }
        text.replace('\\', "/")
    }

    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy().into_owned()
    }
}

fn is_elevated_hint() -> bool {
    env::var("USERNAME")
        .map(|name| name.eq_ignore_ascii_case("administrator"))
        .unwrap_or(false)
}
