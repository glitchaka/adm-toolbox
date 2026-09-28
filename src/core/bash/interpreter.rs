use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};

use super::{
    ast::{AstNode, CaseTerminator, RedirectKind, SimpleCommand},
    environment::ShellEnvironment,
    parse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowSignal {
    None,
    Break(usize),
    Continue(usize),
    Return,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
    pub exit_requested: bool,
    flow: FlowSignal,
    errexit_exempt: bool,
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: u32,
    pub pid: u32,
    pub command: String,
    pub running: bool,
    pub stopped: bool,
}

#[derive(Debug, Clone)]
#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: Option<i64>,
    command: String,
}

struct CallFrame {
    function: String,
    source: String,
    line: u32,
    args: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProcessSubstitution {
    path: PathBuf,
    command: String,
    consume_as_stdin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum OutputSink {
    Stdout,
    Stderr,
    File(PathBuf, bool),
    HostFd(i32),
    Closed,
}


#[derive(Debug, Clone, Default)]
struct CompletionSpec {
    words: Vec<String>,
    action: Option<String>,
    function: Option<String>,
    command: Option<String>,
    prefix: String,
    suffix: String,
    options: HashSet<String>,
}

impl ExecutionResult {
    fn append(&mut self, next: Self) {
        self.stdout.push_str(&next.stdout);
        self.stderr.push_str(&next.stderr);
        self.status = next.status;
        self.exit_requested = next.exit_requested;
        if next.flow != FlowSignal::None {
            self.flow = next.flow;
        }
        self.errexit_exempt = next.errexit_exempt;
    }
    pub fn success() -> Self {
        Self::from_parts(String::new(), String::new(), 0)
    }

    pub fn from_parts(stdout: String, stderr: String, status: i32) -> Self {
        Self {
            stdout,
            stderr,
            status,
            exit_requested: false,
            flow: FlowSignal::None,
            errexit_exempt: false,
        }
    }
}

pub trait ShellCommandHost: Send + Sync {
    fn interrupted(&self) -> bool { false }

    fn read_line(&self, prompt: &str, silent: bool) -> Result<Option<String>> {
        if !prompt.is_empty() {
            eprint!("{prompt}");
            let _ = std::io::stderr().flush();
        }
        let mut line = String::new();
        let read = std::io::stdin().read_line(&mut line)?;
        if read == 0 { return Ok(None); }
        while line.ends_with(['\r', '\n']) { line.pop(); }
        let _ = silent;
        Ok(Some(line))
    }

    fn read_input(
        &self,
        prompt: &str,
        silent: bool,
        delimiter: char,
        max_chars: Option<usize>,
        timeout: Option<std::time::Duration>,
        initial: &str,
    ) -> Result<Option<String>> {
        let _ = (delimiter, max_chars, timeout, initial);
        self.read_line(prompt, silent)
    }

    fn execute_builtin(
        &self,
        name: &str,
        args: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>>;

    fn execute_external(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult>;

    fn execute_external_background(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<u32> {
        let _ = (program, args, cwd, env);
        bail!("background no disponible en este host")
    }

    fn execute_shell_background(
        &self,
        source: &str,
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<u32> {
        let _ = (source, cwd, env);
        bail!("background de shell no disponible en este host")
    }

    fn execute_shell_pipeline(
        &self,
        _commands: &[String],
        _stderr_to_pipe: &[bool],
        _cwd: &Path,
        _env: &HashMap<String, String>,
        _stdin: Option<&[u8]>,
    ) -> Result<Option<(ExecutionResult, Vec<i32>)>> {
        Ok(None)
    }

    fn start_coproc(
        &self,
        _source: &str,
        _cwd: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<Option<(u32, i32, i32)>> {
        Ok(None)
    }

    fn read_fd(
        &self,
        _fd: i32,
        _delimiter: char,
        _max_chars: Option<usize>,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    fn write_fd(&self, _fd: i32, _data: &[u8]) -> Result<bool> { Ok(false) }
    fn close_fd(&self, _fd: i32) -> Result<bool> { Ok(false) }

    /// Returns shell user/system CPU seconds followed by cumulative child
    /// user/system CPU seconds.
    fn process_times(&self) -> Result<(f64, f64, f64, f64)> {
        Ok((0.0, 0.0, 0.0, 0.0))
    }

    fn file_type_test(&self, _path: &Path, _kind: char) -> Result<Option<bool>> {
        Ok(None)
    }

    fn create_process_substitution_pipe(
        &self,
        _direction: char,
        _source: &str,
        _cwd: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<Option<PathBuf>> {
        Ok(None)
    }

    fn fd_is_terminal(&self, fd: i32) -> bool { (0..=2).contains(&fd) }

    fn command_is_builtin(&self, _name: &str) -> bool { false }
    fn command_names(&self) -> Vec<String> { Vec::new() }
    fn user_names(&self) -> Vec<String> { Vec::new() }
    fn group_names(&self) -> Vec<String> { Vec::new() }
    fn jobs(&self) -> Result<Vec<JobInfo>> { Ok(Vec::new()) }
    fn wait_job(&self, _pid: Option<u32>) -> Result<i32> { Ok(127) }
    fn wait_next_job(&self) -> Result<Option<(u32, i32)>> { Ok(None) }
    fn disown_job(&self, _pid: u32) -> Result<bool> { Ok(false) }
    fn signal_process(&self, _pid: u32, _signal: &str) -> Result<bool> { Ok(false) }
}

pub struct Interpreter {
    pub env: ShellEnvironment,
    host: Box<dyn ShellCommandHost>,
    loop_depth: usize,
    source_depth: usize,
    command_hash: HashMap<String, String>,
    function_sources: HashMap<String, String>,
    disabled_builtins: HashSet<String>,
    completion_specs: HashMap<String, CompletionSpec>,
    readline_bindings: HashMap<String, String>,
    process_substitutions: Vec<ProcessSubstitution>,
    process_substitution_counter: u64,
    call_stack: Vec<CallFrame>,
    trap_depth: usize,
    ulimits: HashMap<char, String>,
    checkjobs_warned: bool,
    errexit_suppression: usize,
    persistent_output_routes: HashMap<i32, OutputSink>,
    persist_next_redirections: bool,
}

impl Interpreter {
    pub fn new(host: Box<dyn ShellCommandHost>) -> Self {
        let mut interpreter = Self {
            env: ShellEnvironment::new(),
            host,
            loop_depth: 0,
            source_depth: 0,
            command_hash: HashMap::new(),
            function_sources: HashMap::new(),
            disabled_builtins: HashSet::new(),
            completion_specs: HashMap::new(),
            readline_bindings: HashMap::new(),
            process_substitutions: Vec::new(),
            process_substitution_counter: 0,
            call_stack: Vec::new(),
            trap_depth: 0,
            ulimits: HashMap::new(),
            checkjobs_warned: false,
            errexit_suppression: 0,
            persistent_output_routes: HashMap::new(),
            persist_next_redirections: false,
        };
        interpreter.import_exported_functions();
        interpreter
    }

    fn import_exported_functions(&mut self) {
        let imported = self.env.exported.iter()
            .filter_map(|(name, value)| {
                let function = name.strip_prefix("BASH_FUNC_")?.strip_suffix("%%")?;
                let body = value.strip_prefix("() {")?.trim().strip_suffix('}')?.trim();
                Some((function.to_owned(), body.to_owned()))
            })
            .collect::<Vec<_>>();

        for (name, body) in imported {
            if let Ok(ast) = parse(&body) {
                self.env.functions.insert(name.clone(), ast);
                self.function_sources.insert(name.clone(), "environment".to_owned());
                self.env.exported_functions.insert(name);
            }
        }
    }

    fn execution_environment(&self) -> HashMap<String, String> {
        let mut env = self.env.exported.clone();
        env.retain(|name, _| !name.starts_with("BASH_FUNC_"));
        for name in &self.env.exported_functions {
            if let Some(body) = self.env.functions.get(name) {
                env.insert(
                    format!("BASH_FUNC_{name}%%"),
                    format!("() {{ {}; }}", render_ast(body)),
                );
            }
        }
        env
    }


    pub fn set_interactive(&mut self, interactive: bool) {
        set_shell_option(&mut self.env, "interactive", interactive);
        set_shell_option(&mut self.env, "history", interactive);
        set_shell_option(&mut self.env, "histexpand", interactive);
        set_shell_option(&mut self.env, "monitor", interactive);
        if interactive && !self.env.option_enabled("vi") {
            set_shell_option(&mut self.env, "emacs", true);
        }
    }

    pub fn prepare_prompt(&mut self, continuation: bool) -> Result<(String, String, Option<String>)> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        if !continuation {
            let commands = self.env.array_values("PROMPT_COMMAND");
            if commands.is_empty() {
                let command = self.env.get("PROMPT_COMMAND");
                if !command.is_empty() {
                    let result = self.execute_text(&command)?;
                    stdout.push_str(&result.stdout);
                    stderr.push_str(&result.stderr);
                }
            } else {
                for command in commands {
                    if command.is_empty() { continue; }
                    let result = self.execute_text(&command)?;
                    stdout.push_str(&result.stdout);
                    stderr.push_str(&result.stderr);
                }
            }
        }
        let variable = if continuation { "PS2" } else { "PS1" };
        let raw = self.env.get(variable);
        let prompt = if raw.is_empty() {
            if continuation { Some("> ".to_owned()) } else { None }
        } else {
            Some(self.expand_prompt_text(&raw)?)
        };
        Ok((stdout, stderr, prompt))
    }

    pub fn pre_execute_prompt(&mut self) -> Result<String> {
        let raw = self.env.get("PS0");
        if raw.is_empty() { Ok(String::new()) } else { self.expand_prompt_text(&raw) }
    }

    pub fn input_timeout(&self) -> Option<std::time::Duration> {
        let seconds = self.env.get("TMOUT").parse::<u64>().ok()?;
        (seconds > 0).then(|| std::time::Duration::from_secs(seconds))
    }

    fn expand_prompt_text(&mut self, raw: &str) -> Result<String> {
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '\\' || i + 1 >= chars.len() {
                out.push(chars[i]); i += 1; continue;
            }
            i += 1;
            match chars[i] {
                'a' => out.push('\x07'),
                'd' => out.push_str(&chrono::Local::now().format("%a %b %d").to_string()),
                'e' => out.push('\x1b'),
                'h' => {
                    let host = self.env.get("COMPUTERNAME");
                    out.push_str(host.split('.').next().unwrap_or(&host));
                }
                'H' => out.push_str(&self.env.get("COMPUTERNAME")),
                'j' => out.push_str(&self.host.jobs()?.iter().filter(|job| job.running).count().to_string()),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                's' => out.push_str("bash"),
                't' => out.push_str(&chrono::Local::now().format("%H:%M:%S").to_string()),
                'T' => out.push_str(&chrono::Local::now().format("%I:%M:%S").to_string()),
                '@' => out.push_str(&chrono::Local::now().format("%I:%M %p").to_string()),
                'A' => out.push_str(&chrono::Local::now().format("%H:%M").to_string()),
                'u' => out.push_str(&self.env.get("USERNAME")),
                'v' => out.push_str("5.3"),
                'V' => out.push_str("5.3.0"),
                'w' => {
                    let mut display = self.env.cwd.to_string_lossy().into_owned();
                    let home = self.env.get("HOME");
                    let home = if home.is_empty() { self.env.get("USERPROFILE") } else { home };
                    if !home.is_empty() {
                        let home_path = Path::new(&home);
                        if let Ok(relative) = self.env.cwd.strip_prefix(home_path) {
                            display = if relative.as_os_str().is_empty() {
                                "~".to_owned()
                            } else {
                                format!("~/{}", relative.to_string_lossy().replace('\\', "/"))
                            };
                        }
                    }
                    let trim = self.env.get("PROMPT_DIRTRIM").parse::<usize>().unwrap_or(0);
                    if trim > 0 {
                        let normalized = display.replace('\\', "/");
                        let prefix = if normalized.starts_with("~/") { "~/" } else if normalized.starts_with('/') { "/" } else { "" };
                        let body = normalized.strip_prefix(prefix).unwrap_or(&normalized);
                        let parts = body.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
                        if parts.len() > trim {
                            display = format!("{prefix}.../{}", parts[parts.len() - trim..].join("/"));
                        } else {
                            display = normalized;
                        }
                    }
                    out.push_str(&display);
                },
                'W' => out.push_str(self.env.cwd.file_name().and_then(|name| name.to_str()).unwrap_or("/")),
                '!' | '#' => out.push_str(&(self.history_lines().len() + 1).to_string()),
                '$' => out.push(if self.env.get("EUID") == "0" { '#' } else { '$' }),
                '\\' => out.push('\\'),
                '[' | ']' => {}
                other => { out.push('\\'); out.push(other); }
            }
            i += 1;
        }
        if self.env.option_enabled("promptvars") { self.expand_scalar(&out) } else { Ok(out) }
    }

    pub fn complete_line(&mut self, line: &str, cursor: usize) -> Result<Vec<String>> {
        let cursor = cursor.min(line.len());
        let before = &line[..cursor];
        let start = before.rfind(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')'))
            .map_or(0, |index| index + 1);
        let prefix = &before[start..];
        let words = split_shell_words_relaxed(before).unwrap_or_default();
        let command = words.first().map(String::as_str).unwrap_or(prefix);
        self.env.set("COMP_LINE", line.to_owned());
        self.env.set("COMP_POINT", cursor.to_string());
        self.env.set_array("COMP_WORDS", words.clone());
        self.env.set("COMP_CWORD", words.len().saturating_sub(1).to_string());
        self.env.set("COMP_TYPE", "9");
        self.env.set("COMP_KEY", "9");

        if self.env.option_enabled("hostcomplete") {
            if let Some((left, host_prefix)) = prefix.rsplit_once('@') {
                let mut hosts = host_completion_candidates(host_prefix);
                hosts = hosts.into_iter()
                    .map(|host| format!("{left}@{host}"))
                    .collect();
                if !hosts.is_empty() {
                    return Ok(self.apply_completion_filters(hosts));
                }
            }
        }

        if self.env.option_enabled("no_empty_cmd_completion")
            && before.trim().is_empty()
            && prefix.is_empty()
        {
            return Ok(Vec::new());
        }

        if self.env.option_enabled("progcomp") {
            if let Some(spec) = self.completion_specs.get(command).cloned() {
                return self.generate_completions(&spec, prefix, line);
            }
            if self.env.option_enabled("progcomp_alias") {
                if let Some(alias) = self.env.aliases.get(command).cloned() {
                    if let Ok(alias_words) = split_shell_words_relaxed(&alias) {
                        if let Some(expanded_command) = alias_words.first() {
                            if let Some(spec) = self.completion_specs.get(expanded_command).cloned() {
                                return self.generate_completions(&spec, prefix, line);
                            }
                        }
                    }
                }
            }
        }
        if start == 0 {
            let spec = CompletionSpec { action: Some("command".to_owned()), ..CompletionSpec::default() };
            return self.generate_completions(&spec, prefix, line);
        }
        let values = self.path_completions(prefix, false);
        Ok(self.apply_completion_filters(values))
    }

    pub fn prepare_history(&mut self, line: &str) -> Result<(String, bool)> {
        if !self.env.option_enabled("histexpand") || line.is_empty() {
            return Ok((line.to_owned(), false));
        }
        let history = self.history_lines();
        if history.is_empty() { return Ok((line.to_owned(), false)); }
        if line.starts_with('^') {
            let mut parts = line[1..].splitn(3, '^');
            if let (Some(from), Some(to)) = (parts.next(), parts.next()) {
                return Ok((history.last().cloned().unwrap_or_default().replacen(from, to, 1), false));
            }
        }
        let chars: Vec<char> = line.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        let mut single = false;
        let mut print_only = false;
        while i < chars.len() {
            match chars[i] {
                '\'' => { single = !single; out.push(chars[i]); i += 1; }
                '\\' if i + 1 < chars.len() && chars[i + 1] == '!' => { out.push('!'); i += 2; }
                '!' if !single => {
                    let (mut event, used) = resolve_history_event(&chars[i..], &history)?;
                    i += used;
                    if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == 'p' {
                        print_only = true; i += 2;
                    } else if i < chars.len() && chars[i] == ':' {
                        let (modified, consumed) = apply_history_modifiers(&chars[i..], &event)?;
                        event = modified; i += consumed;
                    }
                    out.push_str(&event);
                }
                ch => { out.push(ch); i += 1; }
            }
        }
        if self.env.option_enabled("histverify") || self.env.option_enabled("histreedit") {
            print_only = true;
        }
        Ok((out, print_only))
    }

    pub fn record_history(&mut self, line: &str) -> Result<()> {
        if line.is_empty() || !self.env.option_enabled("history") { return Ok(()); }
        let line = if self.env.option_enabled("cmdhist")
            && !self.env.option_enabled("lithist")
            && line.contains('\n')
        {
            line.lines()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        } else {
            line.to_owned()
        };
        let line = line.as_str();
        let controls: HashSet<String> = self.env.get("HISTCONTROL").split(':')
            .filter(|value| !value.is_empty()).map(str::to_owned).collect();
        if controls.contains("ignorespace") && line.starts_with(' ') { return Ok(()); }
        let histignore = self.env.get("HISTIGNORE");
        if !histignore.is_empty() && histignore.split(':').any(|pattern| {
            glob::Pattern::new(pattern).map(|compiled| compiled.matches(line)).unwrap_or(false)
        }) { return Ok(()); }

        let mut entries = self.history_entries();
        if controls.contains("ignoredups")
            && entries.last().is_some_and(|last| last.command == line)
        {
            return Ok(());
        }
        if controls.contains("erasedups") {
            entries.retain(|existing| existing.command != line);
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok());
        let entry = HistoryEntry { timestamp, command: line.to_owned() };
        entries.push(entry.clone());
        self.env.set("HISTCMD", entries.len().to_string());

        let histsize = self.env.get("HISTSIZE").parse::<usize>().unwrap_or(500);
        if entries.len() > histsize {
            let remove = entries.len() - histsize;
            entries.drain(..remove);
        }
        let filesize = self.env.get("HISTFILESIZE").parse::<usize>().unwrap_or(histsize);
        if entries.len() > filesize {
            let remove = entries.len() - filesize;
            entries.drain(..remove);
        }

        if self.env.option_enabled("histappend")
            && !controls.contains("erasedups")
        {
            if let Some(path) = self.history_path() {
                if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
                let existing_count = self.history_entries().len();
                if existing_count + 1 <= filesize {
                    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                    if let Some(timestamp) = entry.timestamp {
                        writeln!(file, "#{timestamp}")?;
                    }
                    writeln!(file, "{}", entry.command)?;
                    return Ok(());
                }
            }
        }

        self.save_history_entries(&entries)
    }

    pub fn readline_bindings(&self) -> HashMap<String, String> {
        self.readline_bindings.clone()
    }

    pub fn run_readline_binding(
        &mut self,
        command: &str,
        line: &str,
        cursor: usize,
    ) -> Result<(String, usize, String, String)> {
        self.env.set("READLINE_LINE", line.to_owned());
        self.env.set("READLINE_POINT", cursor.to_string());
        self.env.set("READLINE_MARK", cursor.to_string());
        self.env.set("READLINE_ARGUMENT", "1");
        let result = self.execute_text(command)?;
        let updated = self.env.get("READLINE_LINE");
        let point = self.env.get("READLINE_POINT").parse::<usize>()
            .unwrap_or(updated.len()).min(updated.len());
        Ok((updated, point, result.stdout, result.stderr))
    }

    fn run_trap_action(&mut self, signal: &str, status: i32) -> Result<Option<ExecutionResult>> {
        if self.trap_depth > 0 {
            return Ok(None);
        }
        let Some(action) = self.env.traps.get(signal).cloned() else {
            return Ok(None);
        };
        self.trap_depth += 1;
        let previous_status = self.env.last_status;
        self.env.last_status = status;
        self.env.set("BASH_TRAPSIG", signal_number(signal).to_string());
        let result = self.execute_text(&action);
        self.env.set("BASH_TRAPSIG", "0");
        self.env.last_status = previous_status;
        self.trap_depth = self.trap_depth.saturating_sub(1);
        result.map(Some)
    }

    pub fn execute_text(&mut self, input: &str) -> Result<ExecutionResult> {
        let input = if self.env.option_enabled("interactive")
            && !self.env.option_enabled("interactive_comments")
            && !self.env.option_enabled("interactive-comments")
        {
            preserve_interactive_hashes(input)
        } else {
            input.to_owned()
        };

        if self.env.option_enabled("verbose") && self.trap_depth == 0 {
            eprint!("{input}");
            if !input.ends_with('\n') { eprintln!(); }
        }

        let (prepared, temporary) = self.prepare_heredocs(&input)?;
        let node = parse(&prepared)?;

        let result = if self.env.option_enabled("noexec") {
            ExecutionResult::success()
        } else {
            self.execute(&node, None)?
        };

        for name in temporary {
            self.env.unset(&name);
        }

        if result.status != 0
            && !result.errexit_exempt
            && self.errexit_suppression == 0
        {
            if let Some(mut trap_result) = self.run_trap_action("ERR", result.status)? {
                trap_result.status = result.status;
                return Ok(trap_result);
            }
        }

        let mut result = result;
        if self.env.option_enabled("interactive")
            && self.env.option_enabled("onecmd")
            && self.trap_depth == 0
        {
            result.exit_requested = true;
        }
        Ok(result)
    }

    pub fn execute(&mut self, node: &AstNode, stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        if self.host.interrupted() {
            if let Some(mut trapped) = self.run_trap_action("INT", 130)? {
                if trapped.status == 0 { trapped.status = 130; }
                self.env.last_status = trapped.status;
                return Ok(trapped);
            }
            bail!("comando interrumpido");
        }
        let result = match node {
            AstNode::Empty => ExecutionResult::success(),
            AstNode::Sequence(nodes) => {
                let mut last = ExecutionResult::success();
                for node in nodes {
                    last.append(self.execute(node, stdin)?);
                    if last.exit_requested || last.flow != FlowSignal::None { break; }
                    if last.status != 0
                        && self.env.option_enabled("errexit")
                        && self.errexit_suppression == 0
                        && !last.errexit_exempt
                    {
                        break;
                    }
                }
                last
            }
            AstNode::And(left, right) => {
                let mut left = self.execute_errexit_ignored(left, stdin)?;
                if !left.exit_requested && left.flow == FlowSignal::None && left.status == 0 {
                    left.append(self.execute(right, stdin)?);
                } else {
                    left.errexit_exempt = true;
                }
                left
            }
            AstNode::Or(left, right) => {
                let mut left = self.execute_errexit_ignored(left, stdin)?;
                if !left.exit_requested && left.flow == FlowSignal::None && left.status != 0 {
                    left.append(self.execute(right, stdin)?);
                } else {
                    left.errexit_exempt = true;
                }
                left
            }
            AstNode::Pipeline { parts, stderr_to_pipe } => {
                self.execute_pipeline(parts, stderr_to_pipe, stdin)?
            }
            AstNode::Time { body, posix } => self.execute_timed(body, *posix, stdin)?,
            AstNode::Coproc { name, body } => self.execute_coproc(name.as_deref(), body)?,
            AstNode::Negate(body) => {
                let mut result = self.execute_errexit_ignored(body, stdin)?;
                if !result.exit_requested {
                    result.status = if result.status == 0 { 1 } else { 0 };
                }
                result.errexit_exempt = true;
                result
            }
            AstNode::Background(body) => self.execute_background_node(body)?,
            AstNode::Simple(command) => self.execute_simple(command, stdin)?,
            AstNode::ArrayAssign { name, words } => {
                let append = name.ends_with('+');
                let actual_name = name.trim_end_matches('+').to_owned();
                let values = self.expand_words(words)?;
                if self.env.assoc_arrays.contains_key(&actual_name) {
                    let target = self.env.assoc_arrays.entry(actual_name.clone()).or_default();
                    for value in values {
                        if let Some((key, item)) = parse_array_entry(&value) {
                            target.insert(key, item);
                        }
                    }
                    ExecutionResult::success()
                } else {
                    let mut slots: Vec<Option<String>> = if append {
                        self.env.arrays.get(&actual_name)
                            .map(|values| {
                                let present = self.env.array_present.get(&actual_name);
                                values.iter().enumerate()
                                    .map(|(index, value)| {
                                        present.is_some_and(|indices| indices.contains(&index))
                                            .then(|| value.clone())
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let mut next_index = if append {
                        self.env.max_array_index(&actual_name).map(|index| index + 1).unwrap_or(0)
                    } else {
                        0
                    };

                    for value in values {
                        if let Some((key, item)) = parse_array_entry(&value) {
                            if let Ok(index) = key.parse::<usize>() {
                                if slots.len() <= index { slots.resize(index + 1, None); }
                                slots[index] = Some(item);
                                next_index = index.saturating_add(1);
                            } else {
                                if slots.len() <= next_index { slots.resize(next_index + 1, None); }
                                slots[next_index] = Some(value);
                                next_index += 1;
                            }
                        } else {
                            if slots.len() <= next_index { slots.resize(next_index + 1, None); }
                            slots[next_index] = Some(value);
                            next_index += 1;
                        }
                    }
                    if self.env.set_sparse_array(actual_name.clone(), slots) {
                        ExecutionResult::success()
                    } else {
                        ExecutionResult::from_parts(
                            String::new(),
                            format!("{actual_name}: variable de solo lectura\n"),
                            1,
                        )
                    }
                }
            }
            AstNode::If { condition, then_branch, else_branch } => {
                let mut condition = self.execute_errexit_ignored(condition, stdin)?;
                if condition.exit_requested { condition }
                else if condition.status == 0 {
                    condition.append(self.execute(then_branch, stdin)?);
                    condition
                } else if let Some(branch) = else_branch {
                    condition.append(self.execute(branch, stdin)?);
                    condition
                } else {
                    condition.status = 0;
                    condition
                }
            }
            AstNode::For { name, words, body } => {
                let values = if words.is_empty() {
                    self.env.positional.clone()
                } else {
                    self.expand_words(words)?
                };
                let mut last = ExecutionResult::success();
                self.loop_depth += 1;
                for value in values {
                    self.env.set(name.clone(), value);
                    last.append(self.execute(body, stdin)?);
                    if last.exit_requested { break; }
                    match last.flow {
                        FlowSignal::Break(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Break(levels - 1) } else { FlowSignal::None };
                            break;
                        }
                        FlowSignal::Continue(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Continue(levels - 1) } else { FlowSignal::None };
                            if levels > 1 { break; }
                            continue;
                        }
                        FlowSignal::Return => break,
                        FlowSignal::None => {}
                    }
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                last
            }
            AstNode::ArithmeticFor { init, condition, update, body } => {
                if !init.trim().is_empty() {
                    let _ = self.evaluate_arithmetic_command(init)?;
                }
                let mut last = ExecutionResult::success();
                self.loop_depth += 1;
                loop {
                    if !condition.trim().is_empty() && self.evaluate_arithmetic_command(condition)? == 0 {
                        break;
                    }

                    last.append(self.execute(body, stdin)?);
                    if last.exit_requested { break; }

                    match last.flow {
                        FlowSignal::Break(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Break(levels - 1) } else { FlowSignal::None };
                            break;
                        }
                        FlowSignal::Continue(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Continue(levels - 1) } else { FlowSignal::None };
                            if levels > 1 { break; }
                        }
                        FlowSignal::Return => break,
                        FlowSignal::None => {}
                    }

                    if !update.trim().is_empty() {
                        let _ = self.evaluate_arithmetic_command(update)?;
                    }
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                last
            }
            AstNode::Select { name, words, body } => {
                let values = if words.is_empty() { self.env.positional.clone() } else { self.expand_words(words)? };
                let mut last = ExecutionResult::success();
                self.loop_depth += 1;
                loop {
                    let mut menu = String::new();
                    for (index, value) in values.iter().enumerate() {
                        menu.push_str(&format!("{} ) {}\n", index + 1, value));
                    }
                    let prompt = self.env.vars.get("PS3").cloned().unwrap_or_else(|| "#? ".to_owned());
                    let combined_prompt = format!("{menu}{prompt}");
                    let Some(answer) = self.host.read_line(&combined_prompt, false)? else { break; };
                    self.env.set("REPLY", answer.clone());
                    let selected = answer.trim().parse::<usize>().ok()
                        .and_then(|i| i.checked_sub(1))
                        .and_then(|i| values.get(i))
                        .cloned()
                        .unwrap_or_default();
                    self.env.set(name.clone(), selected);

                    last.append(self.execute(body, stdin)?);
                    if last.exit_requested { break; }
                    match last.flow {
                        FlowSignal::Break(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Break(levels - 1) } else { FlowSignal::None };
                            break;
                        }
                        FlowSignal::Continue(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Continue(levels - 1) } else { FlowSignal::None };
                            if levels > 1 { break; }
                        }
                        FlowSignal::Return => break,
                        FlowSignal::None => {}
                    }
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                last
            }
            AstNode::While { condition, body, until } => {
                let mut last = ExecutionResult::success();
                self.loop_depth += 1;
                loop {
                    let condition_result = self.execute_errexit_ignored(condition, None)?;
                    let should_run = if *until {
                        condition_result.status != 0
                    } else {
                        condition_result.status == 0
                    };
                    if !should_run { break; }

                    last.append(self.execute(body, stdin)?);
                    if last.exit_requested { break; }

                    match last.flow {
                        FlowSignal::Break(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Break(levels - 1) } else { FlowSignal::None };
                            break;
                        }
                        FlowSignal::Continue(levels) => {
                            last.flow = if levels > 1 { FlowSignal::Continue(levels - 1) } else { FlowSignal::None };
                            if levels > 1 { break; }
                            continue;
                        }
                        FlowSignal::Return => break,
                        FlowSignal::None => {}
                    }
                }
                self.loop_depth = self.loop_depth.saturating_sub(1);
                last
            }
            AstNode::Case { word, arms } => {
                let value = self.expand_scalar(word)?;
                let mut result = ExecutionResult::success();
                let mut force_next = false;
                let mut continue_matching = false;

                for arm in arms {
                    let matched = force_next || arm.patterns.iter().any(|pattern| {
                        let pattern = self.expand_scalar(pattern).unwrap_or_else(|_| pattern.clone());
                        glob::Pattern::new(&pattern)
                            .map(|candidate| candidate.matches(&value))
                            .unwrap_or(false)
                    });

                    if matched {
                        result = self.execute(&arm.body, stdin)?;
                        if result.exit_requested || result.flow != FlowSignal::None {
                            break;
                        }
                        match arm.terminator {
                            CaseTerminator::Break => break,
                            CaseTerminator::Fallthrough => {
                                force_next = true;
                                continue_matching = false;
                            }
                            CaseTerminator::ContinueMatching => {
                                force_next = false;
                                continue_matching = true;
                            }
                        }
                    } else if continue_matching {
                        continue;
                    }
                }
                result
            }
            AstNode::Conditional(expression) => {
                let success = self.evaluate_conditional(expression)?;
                ExecutionResult::from_parts(String::new(), String::new(), if success { 0 } else { 1 })
            }
            AstNode::ArithmeticCommand(expression) => {
                let value = self.evaluate_arithmetic_command(expression)?;
                ExecutionResult::from_parts(String::new(), String::new(), if value != 0 { 0 } else { 1 })
            }
            AstNode::FunctionDef { name, body } => {
                if self.env.readonly_functions.contains(name) {
                    ExecutionResult::from_parts(
                        String::new(),
                        format!("{name}: función de solo lectura\n"),
                        1,
                    )
                } else {
                    self.env.functions.insert(name.clone(), (**body).clone());
                    let source = if self.env.option_enabled("bash_source_fullpath") {
                        let path = PathBuf::from(&self.env.script_name);
                        (if path.is_absolute() { path } else { self.env.cwd.join(path) })
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        self.env.script_name.clone()
                    };
                    self.function_sources.insert(name.clone(), source);
                    ExecutionResult::success()
                }
            }
            AstNode::Group(body) => self.execute(body, stdin)?,
            AstNode::Subshell(body) => {
                let saved = self.env.clone();
                let saved_routes = self.persistent_output_routes.clone();
                let saved_persist = self.persist_next_redirections;
                let depth = self.env.get("BASH_SUBSHELL")
                    .parse::<u32>()
                    .unwrap_or(0)
                    .saturating_add(1);
                self.env.set("BASH_SUBSHELL", depth.to_string());
                let result = self.execute(body, stdin);
                self.env = saved;
                self.persistent_output_routes = saved_routes;
                self.persist_next_redirections = saved_persist;
                let mut result = result?;
                result.exit_requested = false;
                result.flow = FlowSignal::None;
                result
            }
        };

        self.env.last_status = result.status;
        Ok(result)
    }

    fn execute_errexit_ignored(
        &mut self,
        node: &AstNode,
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        self.errexit_suppression += 1;
        let result = self.execute(node, stdin);
        self.errexit_suppression = self.errexit_suppression.saturating_sub(1);
        let mut result = result?;
        result.errexit_exempt = true;
        Ok(result)
    }

    fn prepare_heredocs(&mut self, input: &str) -> Result<(String, Vec<String>)> {
        let lines: Vec<&str> = input.split('\n').collect();
        let mut output = String::new();
        let mut temporary = Vec::new();
        let mut index = 0usize;
        let mut counter = 0usize;

        while index < lines.len() {
            let line = lines[index];
            let specs = heredoc_specs(line);
            if specs.is_empty() {
                output.push_str(line);
                if index + 1 < lines.len() { output.push('\n'); }
                index += 1;
                continue;
            }

            let mut rewritten = line.to_owned();
            let mut body_cursor = index + 1;
            let mut replacements = Vec::new();

            for spec in &specs {
                let mut body = String::new();
                let mut found = false;
                while body_cursor < lines.len() {
                    let candidate = if spec.strip_tabs {
                        lines[body_cursor].trim_start_matches('\t')
                    } else {
                        lines[body_cursor]
                    };
                    if candidate == spec.delimiter {
                        found = true;
                        body_cursor += 1;
                        break;
                    }
                    if !body.is_empty() { body.push('\n'); }
                    body.push_str(candidate);
                    body_cursor += 1;
                }
                if !found {
                    bail!("here-document sin delimitador final '{}'", spec.delimiter);
                }

                if !spec.quoted {
                    body = self.expand_scalar(&body)?;
                }

                counter += 1;
                let variable = format!("__SST_HEREDOC_{counter}");
                self.env.set(variable.clone(), body);
                temporary.push(variable.clone());
                replacements.push((spec.start, spec.end, format!("<<< \"${}\"", variable)));
            }

            for (start, end, replacement) in replacements.into_iter().rev() {
                rewritten.replace_range(start..end, &replacement);
            }

            output.push_str(&rewritten);
            if body_cursor < lines.len() { output.push('\n'); }
            index = body_cursor;
        }

        Ok((output, temporary))
    }

    fn execute_background_node(&mut self, node: &AstNode) -> Result<ExecutionResult> {
        // A plain external command can be launched directly by the host. Shell
        // constructs (pipelines, functions, builtins, redirects, groups, etc.)
        // still go through a child Shell Shock Tool interpreter so Bash semantics
        // remain centralized in this module.
        let pid = if let AstNode::Simple(command) = node {
            if command.redirects.is_empty() {
                let mut raw = command.words.clone();
                if let Some(alias) = raw.first()
                    .and_then(|name| self.env.aliases.get(name))
                    .cloned()
                {
                    let mut alias_words = super::lexer::lex(&alias)?
                        .into_iter()
                        .filter_map(|token| {
                            if let super::lexer::Token::Word(word) = token { Some(word) } else { None }
                        })
                        .collect::<Vec<_>>();
                    alias_words.extend(raw.into_iter().skip(1));
                    raw = alias_words;
                }

                let mut index = 0usize;
                let mut child_env = self.execution_environment();
                while index < raw.len() && is_assignment(&raw[index]) {
                    let (name, value) = raw[index].split_once('=').unwrap();
                    child_env.insert(name.to_owned(), self.expand_scalar(value)?);
                    index += 1;
                }

                let words = self.expand_words(&raw[index..])?;
                if let Some((name, args)) = words.split_first() {
                    if !self.shell_builtin_name(name)
                        && !self.env.functions.contains_key(name)
                        && !self.host.command_is_builtin(name)
                    {
                        Some(self.host.execute_external_background(
                            name,
                            args,
                            &self.env.cwd,
                            &child_env,
                        )?)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let pid = if let Some(pid) = pid {
            pid
        } else {
            self.host.execute_shell_background(
                &render_ast(node),
                &self.env.cwd,
                &child_env,
            )?
        };

        self.env.last_background_pid = Some(pid);
        let job_number = self.host.jobs()?
            .iter()
            .find(|job| job.pid == pid)
            .map(|job| job.id)
            .unwrap_or(1);
        Ok(ExecutionResult::from_parts(
            format!("[{job_number}] {pid}\n"),
            String::new(),
            0,
        ))
    }

    fn execute_pipeline(
        &mut self,
        parts: &[AstNode],
        stderr_to_pipe: &[bool],
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let use_lastpipe = self.env.option_enabled("lastpipe")
            && !self.env.option_enabled("monitor")
            && parts.len() > 1;
        let commands: Vec<String> = parts.iter().map(render_ast).collect();
        let child_env = self.execution_environment();
        if !use_lastpipe {
            if let Some((mut result, statuses)) = self.host.execute_shell_pipeline(
                &commands,
                stderr_to_pipe,
                &self.env.cwd,
                &child_env,
                stdin,
            )? {
            self.env.set_array(
                "PIPESTATUS",
                statuses.iter().map(ToString::to_string).collect(),
            );
            if self.env.option_enabled("pipefail") {
                if let Some(status) = statuses.iter().rev().copied().find(|status| *status != 0) {
                    result.status = status;
                }
            }
                return Ok(result);
            }
        }

        let mut input = stdin.map(ToOwned::to_owned);
        let mut collected_stderr = String::new();
        let mut statuses = Vec::new();
        let mut last = ExecutionResult::success();

        for (index, part) in parts.iter().enumerate() {
            if use_lastpipe && index + 1 == parts.len() {
                last = self.execute(part, input.as_deref())?;
            } else {
                let saved = self.env.clone();
                let result = self.execute(part, input.as_deref());
                self.env = saved;
                last = result?;
            }
            last.exit_requested = false;
            last.flow = FlowSignal::None;
            statuses.push(last.status);

            let mut pipe_bytes = last.stdout.as_bytes().to_vec();
            if stderr_to_pipe.get(index).copied().unwrap_or(false) {
                pipe_bytes.extend_from_slice(last.stderr.as_bytes());
            } else {
                collected_stderr.push_str(&last.stderr);
            }
            input = Some(pipe_bytes);
        }

        self.env.set_array(
            "PIPESTATUS",
            statuses.iter().map(ToString::to_string).collect(),
        );
        last.stderr = collected_stderr;
        if self.env.option_enabled("pipefail") {
            if let Some(status) = statuses.iter().rev().copied().find(|status| *status != 0) {
                last.status = status;
            }
        }
        Ok(last)
    }

    fn execute_timed(
        &mut self,
        body: &AstNode,
        posix: bool,
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let started = std::time::Instant::now();
        let before = self.host.process_times().unwrap_or((0.0, 0.0, 0.0, 0.0));
        let mut result = self.execute(body, stdin)?;
        let after = self.host.process_times().unwrap_or(before);
        let elapsed = started.elapsed().as_secs_f64();

        let user = (after.0 - before.0).max(0.0) + (after.2 - before.2).max(0.0);
        let system = (after.1 - before.1).max(0.0) + (after.3 - before.3).max(0.0);

        let timing = if posix {
            format!("real {:.3}\nuser {:.3}\nsys {:.3}\n", elapsed, user, system)
        } else {
            let format = self.env.get("TIMEFORMAT");
            if format.is_empty() {
                format!("\nreal\t{elapsed:.3}s\nuser\t{user:.3}s\nsys\t{system:.3}s\n")
            } else {
                render_timeformat(&format, elapsed, user, system)
            }
        };
        result.stderr.push_str(&timing);
        Ok(result)
    }

    fn execute_coproc(&mut self, name: Option<&str>, body: &AstNode) -> Result<ExecutionResult> {
        let source = render_ast(body);
        let variable = name.unwrap_or("COPROC");
        let child_env = self.execution_environment();

        if let Some((pid, read_fd, write_fd)) = self.host.start_coproc(
            &source,
            &self.env.cwd,
            &child_env,
        )? {
            self.env.last_background_pid = Some(pid);
            self.env.set(format!("{variable}_PID"), pid.to_string());
            self.env.set_array(
                variable.to_owned(),
                vec![read_fd.to_string(), write_fd.to_string()],
            );
            return Ok(ExecutionResult::success());
        }

        let pid = self.host.execute_shell_background(&source, &self.env.cwd, &self.env.exported)?;
        self.env.last_background_pid = Some(pid);
        self.env.set(format!("{variable}_PID"), pid.to_string());
        self.env.set_array(variable.to_owned(), vec![String::new(), String::new()]);
        Ok(ExecutionResult::success())
    }

    fn execute_simple(&mut self, command: &SimpleCommand, stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut local_stdin = stdin.map(ToOwned::to_owned);

        if self.env.option_enabled("restricted_shell")
            && command.redirects.iter().any(|redirect| matches!(
                redirect.kind,
                RedirectKind::Write
                    | RedirectKind::Append
                    | RedirectKind::ReadWrite
                    | RedirectKind::Clobber
                    | RedirectKind::BothWrite
                    | RedirectKind::BothAppend
                    | RedirectKind::DupOutput
            ))
        {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "bash: modo restringido: redirección de salida no permitida\n".to_owned(),
                1,
            ));
        }

        for redirect in &command.redirects {
            match redirect.kind {
                RedirectKind::Read | RedirectKind::ReadWrite => {
                    let target = self.expand_scalar(&redirect.target)?;
                    let path = self.resolve_path(&target);
                    if redirect.kind == RedirectKind::ReadWrite && !path.exists() {
                        OpenOptions::new().create(true).write(true).open(&path)?;
                    }
                    local_stdin = Some(fs::read(path)?);
                }
                RedirectKind::DupInput => {
                    let target = self.expand_scalar(&redirect.target)?;
                    match target.as_str() {
                        "-" => local_stdin = Some(Vec::new()),
                        "0" => {}
                        _ => {
                            let Some(fd) = target.parse::<i32>().ok() else {
                                return Ok(ExecutionResult::from_parts(
                                    String::new(),
                                    format!("{}<&{}: descriptor inválido\n", redirect.fd, target),
                                    1,
                                ));
                            };
                            match self.host.read_fd(fd, '\0', None, None)? {
                                Some(value) => local_stdin = Some(value.into_bytes()),
                                None => return Ok(ExecutionResult::from_parts(
                                    String::new(),
                                    format!("{}<&{}: descriptor no disponible\n", redirect.fd, target),
                                    1,
                                )),
                            }
                        }
                    }
                }
                RedirectKind::HereString => {
                    let mut value = self.expand_scalar(&redirect.target)?;
                    value.push('\n');
                    local_stdin = Some(value.into_bytes());
                }
                _ => {}
            }
        }

        if command.words.is_empty() {
            return Ok(ExecutionResult::success());
        }

        let mut raw = command.words.clone();
        if (self.env.option_enabled("interactive") || self.env.option_enabled("expand_aliases"))
            && let Some(alias) = self.env.aliases.get(&raw[0]).cloned()
        {
            let alias_tokens = super::lexer::lex(&alias)?;
            let mut alias_words = alias_tokens.into_iter().filter_map(|token| {
                if let super::lexer::Token::Word(word) = token { Some(word) } else { None }
            }).collect::<Vec<_>>();
            alias_words.extend(raw.into_iter().skip(1));
            raw = alias_words;
        }

        let mut index = 0;
        while index < raw.len() && is_assignment(&raw[index]) {
            let (name, value) = raw[index].split_once('=').unwrap();
            let value = self.expand_scalar(value)?;
            if !self.env.set(name.to_owned(), value) {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("{name}: asignación no permitida o variable de solo lectura\n"),
                    1,
                ));
            }
            index += 1;
        }

        if index == raw.len() {
            return Ok(ExecutionResult::success());
        }

        let words = self.expand_words(&raw[index..])?;
        if words.is_empty() {
            return Ok(ExecutionResult::success());
        }

        let name = words[0].clone();
        let args = &words[1..];

        if self.env.option_enabled("restricted_shell")
            && (name.contains('/') || name.contains('\\'))
        {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("bash: {name}: modo restringido: no se permite '/' en nombres de comando\n"),
                1,
            ));
        }

        let command_text = words.iter()
            .map(|word| shell_quote(word))
            .collect::<Vec<_>>()
            .join(" ");
        self.env.set("BASH_COMMAND", command_text);
        if let Some(debug_result) = self.run_trap_action("DEBUG", self.env.last_status)? {
            if debug_result.exit_requested || debug_result.flow != FlowSignal::None {
                return Ok(debug_result);
            }
        }

        let mut trace = String::new();
        if self.env.option_enabled("xtrace") {
            let ps4 = self.env.vars.get("PS4").cloned().unwrap_or_else(|| "+ ".to_owned());
            trace.push_str(&ps4);
            trace.push_str(&words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" "));
            trace.push('\n');
        }

        if self.env.option_enabled("autocd")
            && args.is_empty()
            && !self.shell_builtin_name(&name)
            && !self.env.functions.contains_key(&name)
            && !self.host.command_is_builtin(&name)
            && self.resolve_path(&name).is_dir()
        {
            return Ok(self.shell_builtin("cd", &[name.clone()], local_stdin.as_deref())?
                .unwrap_or_else(ExecutionResult::success));
        }

        let mut result = if let Some(result) = self.shell_builtin(&name, args, local_stdin.as_deref())? {
            result
        } else if let Some(body) = self.env.functions.get(&name).cloned() {
            if let Ok(limit) = self.env.get("FUNCNEST").parse::<usize>()
                && limit > 0
                && self.call_stack.len() >= limit
            {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("{name}: profundidad máxima de funciones ({limit}) excedida\n"),
                    1,
                ));
            }
            let saved = self.env.positional.clone();
            self.env.positional = args.to_vec();
            self.env.push_local_scope();
            self.call_stack.push(CallFrame {
                function: name.clone(),
                source: self.function_sources.get(&name)
                    .cloned()
                    .unwrap_or_else(|| self.env.script_name.clone()),
                line: self.env.get("LINENO").parse::<u32>().unwrap_or(0),
                args: args.to_vec(),
            });
            self.sync_call_stack_arrays();
            let execution = self.execute(&body, local_stdin.as_deref());
            let mut result = execution?;
            if result.flow == FlowSignal::Return {
                result.flow = FlowSignal::None;
            }
            if let Some(return_trap) = self.run_trap_action("RETURN", result.status)? {
                result.stdout.push_str(&return_trap.stdout);
                result.stderr.push_str(&return_trap.stderr);
                if return_trap.exit_requested { result.exit_requested = true; }
            }
            self.call_stack.pop();
            self.sync_call_stack_arrays();
            self.env.pop_local_scope();
            self.env.positional = saved;
            result
        } else if let Some(result) = self.host.execute_builtin(
            &name,
            args,
            &self.env.cwd,
            local_stdin.as_deref(),
        )? {
            result
        } else {
            let program = self.resolve_hashed_program(&name)?;
            let child_env = self.execution_environment();
            match self.host.execute_external(
                &program,
                args,
                &self.env.cwd,
                &child_env,
                local_stdin.as_deref(),
            ) {
                Ok(result) => result,
                Err(error) => ExecutionResult::from_parts(
                    String::new(),
                    format!("{name}: {error}\n"),
                    127,
                ),
            }
        };

        if !trace.is_empty() {
            let trace_fd = self.env.get("BASH_XTRACEFD").parse::<i32>().ok().unwrap_or(2);
            if trace_fd == 1 {
                result.stdout = format!("{trace}{}", result.stdout);
            } else if trace_fd == 2 || !self.host.write_fd(trace_fd, trace.as_bytes())? {
                result.stderr = format!("{trace}{}", result.stderr);
            }
        }
        self.apply_output_redirects(command, &mut result)?;
        self.finalize_process_substitutions(&mut result)?;
        Ok(result)
    }

    fn sync_call_stack_arrays(&mut self) {
        let mut functions = self.call_stack.iter().rev()
            .map(|frame| frame.function.clone())
            .collect::<Vec<_>>();
        functions.push("main".to_owned());

        let mut sources = vec![self.env.script_name.clone()];
        sources.extend(self.call_stack.iter().rev().map(|frame| frame.source.clone()));

        let mut lines = self.call_stack.iter().rev()
            .map(|frame| frame.line.to_string())
            .collect::<Vec<_>>();
        lines.push("0".to_owned());

        let mut argc = self.call_stack.iter().rev()
            .map(|frame| frame.args.len().to_string())
            .collect::<Vec<_>>();
        argc.push(self.env.positional.len().to_string());

        let argv = self.call_stack.iter()
            .flat_map(|frame| frame.args.iter().rev().cloned())
            .collect::<Vec<_>>();

        self.env.set_array("FUNCNAME", functions);
        self.env.set_array("BASH_SOURCE", sources);
        self.env.set_array("BASH_LINENO", lines);
        self.env.set_array("BASH_ARGC", argc);
        self.env.set_array("BASH_ARGV", argv);
    }

    fn shell_builtin(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>> {
        if name != "enable" && self.disabled_builtins.contains(name) {
            return Ok(None);
        }
        let result = match name {
            "cd" => {
                if self.env.option_enabled("restricted_shell") {
                    ExecutionResult::from_parts(
                        String::new(),
                        "cd: modo restringido: operación no permitida\n".to_owned(),
                        1,
                    )
                } else {
                    let raw = args.first().map(String::as_str).unwrap_or("~");
                    let mut print_target = raw == "-";
                    let mut target = if raw == "-" {
                        self.env.oldpwd.clone().unwrap_or_else(|| self.env.cwd.clone())
                    } else {
                        self.resolve_path(raw)
                    };

                    if raw != "-"
                        && !raw.contains('/')
                        && !raw.contains('\\')
                        && !target.is_dir()
                    {
                        let cdpath = self.env.get("CDPATH");
                        for entry in cdpath.split(';').flat_map(|chunk| chunk.split(':')) {
                            let base = if entry.is_empty() {
                                self.env.cwd.clone()
                            } else {
                                self.resolve_path(entry)
                            };
                            let candidate = base.join(raw);
                            if candidate.is_dir() {
                                target = candidate;
                                print_target = !entry.is_empty();
                                break;
                            }
                        }
                    }

                    if !target.is_dir() && self.env.option_enabled("cdable_vars") {
                        let variable = self.env.get(raw);
                        if !variable.is_empty() {
                            let candidate = self.resolve_path(&variable);
                            if candidate.is_dir() {
                                target = candidate;
                            }
                        }
                    }

                    if !target.is_dir() && self.env.option_enabled("cdspell") {
                        if let Some(corrected) = self.correct_directory_spelling(raw) {
                            target = corrected;
                            print_target = true;
                        }
                    }

                    match fs::canonicalize(&target) {
                        Ok(path) if path.is_dir() => {
                            let previous = self.env.cwd.clone();
                            self.env.cwd = path.clone();
                            self.env.oldpwd = Some(previous.clone());
                            self.env.set("OLDPWD", previous.to_string_lossy().into_owned());
                            self.env.set("PWD", path.to_string_lossy().into_owned());
                            let stdout = if print_target {
                                format!("{}\n", path.display())
                            } else {
                                String::new()
                            };
                            ExecutionResult::from_parts(stdout, String::new(), 0)
                        }
                        Ok(_) => ExecutionResult::from_parts(
                            String::new(),
                            format!("cd: {raw}: no es un directorio\n"),
                            1,
                        ),
                        Err(error) => ExecutionResult::from_parts(
                            String::new(),
                            format!("cd: {raw}: {error}\n"),
                            1,
                        ),
                    }
                }
            }
            "pwd" => {
                let path = if args.iter().any(|arg| arg == "-P") {
                    fs::canonicalize(&self.env.cwd).unwrap_or_else(|_| self.env.cwd.clone())
                } else {
                    self.env.cwd.clone()
                };
                ExecutionResult::from_parts(format!("{}\n", path.display()), String::new(), 0)
            }
            "echo" => self.builtin_echo(args),
            "printf" => self.builtin_printf(args)?,
            "export" => {
                let functions = args.iter().any(|arg| arg == "-f");
                let unexport = args.iter().any(|arg| arg == "-n");
                let print = args.is_empty() || args.iter().any(|arg| arg == "-p");
                let operands = args.iter().filter(|arg| !arg.starts_with('-')).collect::<Vec<_>>();

                if print {
                    if functions {
                        let mut names = self.env.exported_functions.iter().cloned().collect::<Vec<_>>();
                        names.sort();
                        let mut stdout = String::new();
                        for name in names {
                            if let Some(body) = self.env.functions.get(&name) {
                                stdout.push_str(&format!(
                                    "declare -fx {name}\n{name} () {{\n    {}\n}}\n",
                                    render_ast(body)
                                ));
                            }
                        }
                        ExecutionResult::from_parts(stdout, String::new(), 0)
                    } else {
                        let mut values = self.env.exported.iter()
                            .filter(|(name, _)| !name.starts_with("BASH_FUNC_"))
                            .collect::<Vec<_>>();
                        values.sort_by_key(|(name, _)| *name);
                        let stdout = values.into_iter()
                            .map(|(name, value)| format!("declare -x {}={}\n", name, shell_quote(value)))
                            .collect();
                        ExecutionResult::from_parts(stdout, String::new(), 0)
                    }
                } else {
                    let mut status = 0;
                    let mut stderr = String::new();
                    for arg in operands {
                        if functions {
                            let name = arg.as_str();
                            if !self.env.functions.contains_key(name) {
                                status = 1;
                                stderr.push_str(&format!("export: {name}: no es una función\n"));
                            } else if unexport {
                                self.env.exported_functions.remove(name);
                            } else {
                                self.env.exported_functions.insert(name.to_owned());
                            }
                            continue;
                        }

                        if unexport {
                            let name = arg.split_once('=').map(|(name, _)| name).unwrap_or(arg);
                            self.env.exported.remove(name);
                            continue;
                        }

                        if let Some((name, value)) = arg.split_once('=') {
                            let value = self.expand_scalar(value)?;
                            if !self.env.export(name.to_owned(), value) {
                                status = 1;
                                stderr.push_str(&format!("export: {name}: variable de solo lectura\n"));
                            }
                        } else {
                            self.env.mark_exported(arg);
                        }
                    }
                    ExecutionResult::from_parts(String::new(), stderr, status)
                }
            }
            "unset" => {
                let functions_only = args.iter().any(|arg| arg == "-f");
                let variables_only = args.iter().any(|arg| arg == "-v");
                let nameref_only = args.iter().any(|arg| arg == "-n");
                let mut status = 0;
                let mut stderr = String::new();
                for item in args.iter().filter(|arg| !arg.starts_with('-')) {
                    if nameref_only {
                        if !self.env.unset_nameref(item) {
                            status = 1;
                        }
                        continue;
                    }
                    if !variables_only && self.env.functions.contains_key(item) {
                        if self.env.readonly_functions.contains(item) {
                            status = 1;
                            stderr.push_str(&format!("unset: {item}: función de solo lectura\n"));
                            if functions_only { continue; }
                        } else {
                            self.env.functions.remove(item);
                            self.function_sources.remove(item);
                            self.env.exported_functions.remove(item);
                            if functions_only { continue; }
                        }
                    }
                    if !functions_only && !self.env.unset(item) {
                        status = 1;
                        stderr.push_str(&format!("unset: {item}: variable de solo lectura\n"));
                    }
                }
                ExecutionResult::from_parts(String::new(), stderr, status)
            }
            "alias" => {
                if args.is_empty() {
                    let mut aliases = self.env.aliases.iter().collect::<Vec<_>>();
                    aliases.sort_by_key(|(name, _)| *name);
                    let stdout = aliases.into_iter()
                        .map(|(name, value)| format!("alias {name}={}\n", shell_quote(value)))
                        .collect();
                    ExecutionResult::from_parts(stdout, String::new(), 0)
                } else {
                    let mut stdout = String::new();
                    let mut stderr = String::new();
                    let mut status = 0;
                    for arg in args {
                        if let Some((name, value)) = arg.split_once('=') {
                            self.env.aliases.insert(name.to_owned(), strip_outer_quotes(value));
                        } else if let Some(value) = self.env.aliases.get(arg) {
                            stdout.push_str(&format!("alias {arg}={}\n", shell_quote(value)));
                        } else {
                            stderr.push_str(&format!("alias: {arg}: no encontrado\n"));
                            status = 1;
                        }
                    }
                    ExecutionResult::from_parts(stdout, stderr, status)
                }
            }
            "unalias" => {
                if args.iter().any(|arg| arg == "-a") {
                    self.env.aliases.clear();
                } else {
                    for name in args { self.env.aliases.remove(name); }
                }
                ExecutionResult::success()
            }
            ":" | "true" => ExecutionResult::success(),
            "false" => ExecutionResult::from_parts(String::new(), String::new(), 1),
            "exit" | "logout" => {
                if name == "logout" && !self.env.option_enabled("login_shell") {
                    ExecutionResult::from_parts(
                        String::new(),
                        "logout: no es una shell de login\n".to_owned(),
                        1,
                    )
                } else {
                    let status = args.first()
                        .and_then(|value| value.parse::<i32>().ok())
                        .unwrap_or(self.env.last_status);

                    let running_jobs = self.host.jobs()?
                        .into_iter()
                        .filter(|job| job.running)
                        .collect::<Vec<_>>();

                    if self.env.option_enabled("interactive")
                        && self.env.option_enabled("checkjobs")
                        && !running_jobs.is_empty()
                        && !self.checkjobs_warned
                    {
                        self.checkjobs_warned = true;
                        let mut stderr = "Hay jobs activos.\n".to_owned();
                        for (index, job) in running_jobs.iter().enumerate() {
                            stderr.push_str(&format!(
                                "[{}] Running {} {}\n",
                                index + 1,
                                job.pid,
                                job.command
                            ));
                        }
                        ExecutionResult::from_parts(String::new(), stderr, 1)
                    } else {
                        self.checkjobs_warned = false;

                        if self.env.option_enabled("huponexit")
                            && self.env.option_enabled("login_shell")
                        {
                            for job in running_jobs {
                                let _ = self.host.signal_process(job.pid, "HUP");
                            }
                        }

                        let mut result = ExecutionResult {
                            stdout: String::new(),
                            stderr: String::new(),
                            status,
                            exit_requested: true,
                            flow: FlowSignal::None,
                            errexit_exempt: false,
                        };
                        if let Some(action) = self.env.traps.get("EXIT").cloned().or_else(|| self.env.traps.get("0").cloned()) {
                            let mut trap_result = self.execute_text(&action)?;
                            trap_result.exit_requested = true;
                            trap_result.status = status;
                            result.append(trap_result);
                            result.exit_requested = true;
                            result.status = status;
                        }

                        // Bash login shells read the user and system logout files
                        // when they terminate. Preserve their output but keep the
                        // requested exit status.
                        if self.env.option_enabled("login_shell") {
                            let home = {
                                let home = self.env.get("HOME");
                                if home.is_empty() { self.env.get("USERPROFILE") } else { home }
                            };
                            let mut logout_files = Vec::new();
                            if !home.is_empty() {
                                logout_files.push(PathBuf::from(home).join(".bash_logout"));
                            }
                            logout_files.push(PathBuf::from("/etc/bash.bash_logout"));

                            for path in logout_files {
                                if let Ok(source) = fs::read_to_string(&path) {
                                    let mut logout_result = self.execute_text(&source)?;
                                    logout_result.exit_requested = false;
                                    logout_result.status = status;
                                    result.stdout.push_str(&logout_result.stdout);
                                    result.stderr.push_str(&logout_result.stderr);
                                }
                            }
                        }

                        result
                    }
                }
            }
            "source" | "." => {
                let mut index = 0usize;
                let mut search_path: Option<String> = None;
                if args.first().map(String::as_str) == Some("-p") {
                    let Some(value) = args.get(1) else {
                        return Ok(Some(ExecutionResult::from_parts(
                            String::new(), format!("{name}: -p requiere PATH\n"), 2,
                        )));
                    };
                    search_path = Some(value.clone());
                    index = 2;
                }
                let Some(raw_path) = args.get(index) else {
                    return Ok(Some(ExecutionResult::from_parts(
                        String::new(), format!("{name}: falta archivo\n"), 2,
                    )));
                };

                let direct = self.resolve_path(raw_path);
                let path = if raw_path.contains('/') || raw_path.contains('\\') || direct.is_file() {
                    direct
                } else {
                    let search = search_path
                        .or_else(|| self.env.option_enabled("sourcepath").then(|| self.env.get("PATH")))
                        .unwrap_or_default();
                    std::env::split_paths(&search)
                        .map(|directory| directory.join(raw_path))
                        .find(|candidate| candidate.is_file())
                        .unwrap_or_else(|| self.resolve_path(raw_path))
                };

                let source = match fs::read_to_string(&path) {
                    Ok(source) => source,
                    Err(error) => return Ok(Some(ExecutionResult::from_parts(
                        String::new(),
                        format!("{name}: {}: {error}\n", path.display()),
                        1,
                    ))),
                };

                let saved_positional = self.env.positional.clone();
                let saved_name = self.env.script_name.clone();
                let source_args = &args[index + 1..];
                if !source_args.is_empty() {
                    self.env.positional = source_args.to_vec();
                }
                self.env.script_name = path.to_string_lossy().into_owned();
                self.source_depth += 1;
                self.call_stack.push(CallFrame {
                    function: "source".to_owned(),
                    source: saved_name.clone(),
                    line: self.env.get("LINENO").parse::<u32>().unwrap_or(0),
                    args: source_args.to_vec(),
                });
                self.sync_call_stack_arrays();
                let execution = self.execute_text(&source);
                let mut result = execution?;
                if result.flow == FlowSignal::Return { result.flow = FlowSignal::None; }
                if let Some(return_trap) = self.run_trap_action("RETURN", result.status)? {
                    result.stdout.push_str(&return_trap.stdout);
                    result.stderr.push_str(&return_trap.stderr);
                }
                self.call_stack.pop();
                self.source_depth = self.source_depth.saturating_sub(1);
                self.env.positional = saved_positional;
                self.env.script_name = saved_name;
                self.sync_call_stack_arrays();
                result
            }
            "read" => self.builtin_read(args, stdin)?,
            "local" => {
                if self.env.local_scopes.is_empty() {
                    ExecutionResult::from_parts(String::new(), "local: solo puede usarse dentro de una función\n".to_owned(), 1)
                } else {
                    self.builtin_declare(args, true)?
                }
            }
            "declare" | "typeset" => self.builtin_declare(args, false)?,
            "readonly" => {
                let functions = args.iter().any(|arg| arg == "-f");
                let indexed = args.iter().any(|arg| arg == "-a");
                let associative = args.iter().any(|arg| arg == "-A");
                let print = args.is_empty() || args.iter().any(|arg| arg == "-p");
                let operands = args.iter().filter(|arg| !arg.starts_with('-')).collect::<Vec<_>>();

                if functions {
                    if print || operands.is_empty() {
                        let mut names = self.env.readonly_functions.iter().cloned().collect::<Vec<_>>();
                        names.sort();
                        let stdout = names.into_iter()
                            .map(|name| format!("declare -fr {name}\n"))
                            .collect();
                        ExecutionResult::from_parts(stdout, String::new(), 0)
                    } else {
                        let mut status = 0;
                        let mut stderr = String::new();
                        for name in operands {
                            if self.env.functions.contains_key(name.as_str()) {
                                self.env.readonly_functions.insert(name.to_string());
                            } else {
                                status = 1;
                                stderr.push_str(&format!("readonly: {name}: no es una función\n"));
                            }
                        }
                        ExecutionResult::from_parts(String::new(), stderr, status)
                    }
                } else if print {
                    let mut names = self.env.readonly.iter().cloned().collect::<Vec<_>>();
                    names.sort();
                    let mut stdout = String::new();
                    for name in names {
                        if self.env.arrays.contains_key(&name) {
                            stdout.push_str(&format!("declare -ar {name}\n"));
                        } else if self.env.assoc_arrays.contains_key(&name) {
                            stdout.push_str(&format!("declare -Ar {name}\n"));
                        } else {
                            stdout.push_str(&format!("declare -r {}={}\n", name, shell_quote(&self.env.get(&name))));
                        }
                    }
                    ExecutionResult::from_parts(stdout, String::new(), 0)
                } else {
                    let mut status = 0;
                    let mut stderr = String::new();
                    for arg in operands {
                        let (name, value) = arg.split_once('=')
                            .map(|(name, value)| (name.to_owned(), Some(value.to_owned())))
                            .unwrap_or_else(|| (arg.to_string(), None));

                        if indexed && !self.env.arrays.contains_key(&name) {
                            self.env.set_array(name.clone(), Vec::new());
                        }
                        if associative && !self.env.assoc_arrays.contains_key(&name) {
                            self.env.declare_assoc(name.clone());
                        }
                        if let Some(value) = value {
                            let expanded = self.expand_scalar(&value)?;
                            if !self.env.set(name.clone(), expanded) {
                                status = 1;
                                stderr.push_str(&format!("readonly: {name}: variable de solo lectura\n"));
                                continue;
                            }
                        }
                        self.env.set_readonly(&name);
                    }
                    ExecutionResult::from_parts(String::new(), stderr, status)
                }
            }
            "break" => {
                if self.loop_depth == 0 {
                    ExecutionResult::from_parts(String::new(), "break: solo puede usarse dentro de un bucle\n".to_owned(), 1)
                } else {
                    let levels = args.first().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                    let mut result = ExecutionResult::success();
                    result.flow = FlowSignal::Break(levels.min(self.loop_depth));
                    result
                }
            }
            "continue" => {
                if self.loop_depth == 0 {
                    ExecutionResult::from_parts(String::new(), "continue: solo puede usarse dentro de un bucle\n".to_owned(), 1)
                } else {
                    let levels = args.first().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                    let mut result = ExecutionResult::success();
                    result.flow = FlowSignal::Continue(levels.min(self.loop_depth));
                    result
                }
            }
            "return" => {
                let status = args.first().and_then(|value| value.parse::<i32>().ok()).unwrap_or(self.env.last_status);
                if self.env.local_scopes.is_empty() && self.source_depth == 0 {
                    ExecutionResult::from_parts(String::new(), "return: solo puede usarse dentro de una función o script sourced\n".to_owned(), 1)
                } else {
                    let mut result = ExecutionResult::from_parts(String::new(), String::new(), status);
                    result.flow = FlowSignal::Return;
                    result
                }
            }
            "shift" => {
                let count = args.first().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                if count > self.env.positional.len() {
                    let stderr = if self.env.option_enabled("shift_verbose") {
                        format!("shift: {count}: cantidad fuera de rango\n")
                    } else {
                        String::new()
                    };
                    ExecutionResult::from_parts(String::new(), stderr, 1)
                } else {
                    self.env.positional.drain(..count);
                    ExecutionResult::success()
                }
            }
            "set" => self.builtin_set(args)?,
            "shopt" => self.builtin_shopt(args),
            "trap" => self.builtin_trap(args),
            "eval" => {
                let source = args.join(" ");
                self.execute_text(&source)?
            }
            "let" => {
                let mut value = 0;
                for expression in args {
                    value = self.evaluate_arithmetic_command(expression)?;
                }
                ExecutionResult::from_parts(String::new(), String::new(), if value == 0 { 1 } else { 0 })
            }
            "test" | "[" => {
                let mut expr = args.to_vec();
                if name == "[" {
                    if expr.last().map(String::as_str) != Some("]") {
                        ExecutionResult::from_parts(String::new(), "[: falta ']'\n".to_owned(), 2)
                    } else {
                        expr.pop();
                        let success = self.evaluate_conditional(&expr)?;
                        ExecutionResult::from_parts(String::new(), String::new(), if success { 0 } else { 1 })
                    }
                } else {
                    let success = self.evaluate_conditional(&expr)?;
                    ExecutionResult::from_parts(String::new(), String::new(), if success { 0 } else { 1 })
                }
            }
            "mapfile" | "readarray" => self.builtin_mapfile(args, stdin)?,
            "help" => self.builtin_help(args),
            "kill" => self.builtin_kill(args)?,
            "jobs" => self.builtin_jobs(args)?,
            "wait" => self.builtin_wait(args)?,
            "fg" => self.builtin_fg(args)?,
            "bg" => self.builtin_bg(args)?,
            "disown" => self.builtin_disown(args)?,
            "command" => self.builtin_command(args, stdin)?,
            "builtin" => {
                let Some(command) = args.first() else { return Ok(Some(ExecutionResult::success())); };
                self.shell_builtin(command, &args[1..], stdin)?
                    .unwrap_or_else(|| ExecutionResult::from_parts(String::new(), format!("builtin: {command}: no es builtin\n"), 1))
            }
            "type" => self.builtin_type(args)?,
            "hash" => self.builtin_hash(args)?,
            "getopts" => self.builtin_getopts(args)?,
            "exec" => self.builtin_exec(args, stdin)?,
            "history" => self.builtin_history(args)?,
            "fc" => self.builtin_fc(args)?,
            "bind" => self.builtin_bind(args)?,
            "enable" => self.builtin_enable(args)?,
            "complete" => self.builtin_complete(args)?,
            "compgen" => self.builtin_compgen(args)?,
            "compopt" => self.builtin_compopt(args)?,
            "suspend" => self.builtin_suspend(args)?,
            "dirs" => self.builtin_dirs(args)?,
            "pushd" => self.builtin_pushd(args)?,
            "popd" => self.builtin_popd(args)?,
            "umask" => self.builtin_umask(args),
            "ulimit" => self.builtin_ulimit(args)?,
            "times" => self.builtin_times(),
            "caller" => self.builtin_caller(args),
            "xargs" => self.execute_xargs(args, stdin)?,
            "config" => {
                match args.first().map(String::as_str).unwrap_or("path") {
                    "path" => ExecutionResult::from_parts(format!("{}\n", self.env.get("ADM_CONFIG")), String::new(), 0),
                    "reload" => {
                        let path = self.env.get("ADM_CONFIG");
                        if path.is_empty() {
                            ExecutionResult::from_parts(String::new(), "config: ADM_CONFIG no definido\n".to_owned(), 1)
                        } else {
                            self.execute_text(&fs::read_to_string(path)?)?
                        }
                    }
                    "edit" => {
                        let edit_args = vec!["edit".to_owned()];
                        self.host.execute_builtin("adm-config", &edit_args, &self.env.cwd, stdin)?
                            .unwrap_or_else(|| ExecutionResult::from_parts(String::new(), "config edit: builtin no disponible\n".to_owned(), 127))
                    }
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };

        Ok(Some(result))
    }

    fn shell_builtin_name(&self, name: &str) -> bool {
        if self.disabled_builtins.contains(name) { return false; }
        matches!(
            name,
            "cd" | "pwd" | "echo" | "printf" | "export" | "unset" | "alias" | "unalias"
                | "exit" | "logout" | "source" | "." | "read" | "local" | "declare" | "typeset"
                | "readonly" | "break" | "continue" | "return" | "shift" | "set" | "shopt"
                | "trap" | "eval" | "let" | "test" | "[" | "mapfile" | "readarray" | "jobs"
                | "wait" | "fg" | "bg" | "disown" | "command" | "builtin" | "type" | "hash" | "getopts"
                | "exec" | "history" | "fc" | "bind" | "enable" | "complete" | "compgen" | "compopt" | "suspend"
                | "dirs" | "pushd" | "popd" | "umask" | "ulimit" | "times" | "caller"
                | "help" | "kill" | ":" | "true" | "false"
        )
    }

    fn builtin_help(&self, args: &[String]) -> ExecutionResult {
        let short = args.iter().any(|arg| arg == "-s");
        let description_only = args.iter().any(|arg| arg == "-d");
        let topics: Vec<&str> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(String::as_str)
            .collect();

        if topics.is_empty() {
            let mut names = bash_builtin_names().to_vec();
            names.sort();
            let mut stdout = String::from("Shell Shock Tool — builtins compatibles con Bash 5.3:\n");
            for chunk in names.chunks(4) {
                for name in chunk {
                    stdout.push_str(&format!("{name:<18}"));
                }
                stdout.push('\n');
            }
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut status = 0;
        for topic in topics {
            let mut matches = bash_builtin_names().iter()
                .copied()
                .filter(|name| glob::Pattern::new(topic)
                    .map(|pattern| pattern.matches(name))
                    .unwrap_or(*name == topic))
                .collect::<Vec<_>>();
            matches.sort();
            matches.dedup();

            if matches.is_empty() {
                stderr.push_str(&format!("help: no hay tema de ayuda para '{topic}'\n"));
                status = 1;
                continue;
            }

            for name in matches {
                let (usage, description) = bash_builtin_help(name);
                if description_only {
                    stdout.push_str(description);
                    stdout.push('\n');
                } else if short {
                    stdout.push_str(usage);
                    stdout.push('\n');
                } else {
                    stdout.push_str(&format!("{name}: {description}\n    {usage}\n"));
                }
            }
        }
        ExecutionResult::from_parts(stdout, stderr, status)
    }

    fn builtin_kill(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.is_empty() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "kill: uso: kill [-s señal | -n señal | -señal] pid | %job ...\n".to_owned(),
                2,
            ));
        }

        if args.iter().any(|arg| matches!(arg.as_str(), "-l" | "-L")) {
            let values: Vec<&String> = args.iter()
                .filter(|arg| !matches!(arg.as_str(), "-l" | "-L"))
                .collect();
            if values.is_empty() {
                let stdout = bash_signal_names()
                    .iter()
                    .enumerate()
                    .map(|(index, name)| format!("{:2}) SIG{}\n", index + 1, name))
                    .collect();
                return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
            }

            let mut stdout = String::new();
            let mut status = 0;
            for value in values {
                if let Ok(number) = value.parse::<usize>() {
                    let normalized = if number > 128 { number - 128 } else { number };
                    if let Some(name) = bash_signal_names().get(normalized.saturating_sub(1)) {
                        stdout.push_str(name);
                        stdout.push('\n');
                    } else {
                        status = 1;
                    }
                } else {
                    let normalized = normalize_signal(value);
                    let number = signal_number(&normalized);
                    if number == 0 {
                        status = 1;
                    } else {
                        stdout.push_str(&number.to_string());
                        stdout.push('\n');
                    }
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), status));
        }

        let mut signal = "TERM".to_owned();
        let mut targets = Vec::new();
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "-s" | "-n" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "kill: falta especificación de señal\n".to_owned(),
                            2,
                        ));
                    };
                    signal = normalize_signal(value);
                }
                "--" => {
                    targets.extend(args[index + 1..].iter().cloned());
                    break;
                }
                value if value.starts_with('-') && value.len() > 1 => {
                    signal = normalize_signal(&value[1..]);
                }
                value => targets.push(value.to_owned()),
            }
            index += 1;
        }

        if signal != "0" && signal_number(&signal) == 0 {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("kill: {signal}: señal inválida\n"),
                2,
            ));
        }
        if targets.is_empty() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "kill: falta pid o jobspec\n".to_owned(),
                2,
            ));
        }

        let mut stderr = String::new();
        let mut status = 0;
        for target in targets {
            let pid = if target.starts_with('%') {
                match self.resolve_jobspec(Some(&target))? {
                    Some((_, job)) => job.pid,
                    None => {
                        stderr.push_str(&format!("kill: {target}: no existe ese job\n"));
                        status = 1;
                        continue;
                    }
                }
            } else {
                match target.parse::<u32>() {
                    Ok(pid) => pid,
                    Err(_) => {
                        stderr.push_str(&format!("kill: {target}: argumentos deben ser pid o jobspec\n"));
                        status = 1;
                        continue;
                    }
                }
            };

            if !self.host.signal_process(pid, &signal)? {
                stderr.push_str(&format!("kill: ({pid}) - no existe el proceso o no se pudo señalizar\n"));
                status = 1;
            }
        }

        Ok(ExecutionResult::from_parts(String::new(), stderr, status))
    }

    fn resolve_jobspec(&self, spec: Option<&str>) -> Result<Option<(u32, JobInfo)>> {
        let jobs = self.host.jobs()?;
        if jobs.is_empty() {
            return Ok(None);
        }

        let selected = match spec {
            None | Some("%") | Some("%%") | Some("%+") => jobs.last().cloned(),
            Some("%-") => jobs.get(jobs.len().saturating_sub(2)).cloned(),
            Some(value) if value.starts_with("%?") => {
                let needle = &value[2..];
                let matches = jobs.iter()
                    .filter(|job| job.command.contains(needle))
                    .cloned()
                    .collect::<Vec<_>>();
                (matches.len() == 1).then(|| matches[0].clone())
            }
            Some(value) if value.starts_with('%') => {
                let tail = &value[1..];
                if let Ok(number) = tail.parse::<u32>() {
                    jobs.iter().find(|job| job.id == number).cloned()
                } else {
                    let matches = jobs.iter()
                        .filter(|job| job.command.starts_with(tail))
                        .cloned()
                        .collect::<Vec<_>>();
                    (matches.len() == 1).then(|| matches[0].clone())
                }
            }
            Some(value) => value.parse::<u32>().ok()
                .and_then(|pid| jobs.iter().find(|job| job.pid == pid).cloned()),
        };

        Ok(selected.map(|job| (job.id, job)))
    }

    fn builtin_jobs(&self, args: &[String]) -> Result<ExecutionResult> {
        let jobs = self.host.jobs()?;
        let pids_only = args.iter().any(|arg| arg == "-p");
        let long = args.iter().any(|arg| arg == "-l");
        let running_only = args.iter().any(|arg| arg == "-r");
        let stopped_only = args.iter().any(|arg| arg == "-s");
        let requested: Vec<&str> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(String::as_str)
            .collect();

        let current_id = jobs.last().map(|job| job.id);
        let previous_id = jobs.get(jobs.len().saturating_sub(2)).map(|job| job.id);
        let mut stdout = String::new();

        for job in &jobs {
            if running_only && !job.running { continue; }
            if stopped_only && !job.stopped { continue; }
            if !requested.is_empty() && !requested.iter().any(|spec| {
                if **spec == "%+" || **spec == "%%" || **spec == "%" {
                    return current_id == Some(job.id);
                }
                if **spec == "%-" {
                    return previous_id == Some(job.id);
                }
                if let Some(needle) = spec.strip_prefix("%?") {
                    return job.command.contains(needle);
                }
                if let Some(tail) = spec.strip_prefix('%') {
                    return tail.parse::<u32>().ok() == Some(job.id)
                        || job.command.starts_with(tail);
                }
                spec.parse::<u32>().ok() == Some(job.pid)
            }) {
                continue;
            }

            if pids_only {
                stdout.push_str(&format!("{}\n", job.pid));
                continue;
            }

            let marker = if current_id == Some(job.id) {
                '+'
            } else if previous_id == Some(job.id) {
                '-'
            } else {
                ' '
            };
            let state = if job.stopped {
                "Stopped"
            } else if job.running {
                "Running"
            } else {
                "Done"
            };

            if long {
                stdout.push_str(&format!(
                    "[{}]{} {:>6} {:<8} {}\n",
                    job.id, marker, job.pid, state, job.command
                ));
            } else {
                stdout.push_str(&format!(
                    "[{}]{} {:<8} {}\n",
                    job.id, marker, state, job.command
                ));
            }
        }

        // Bash 5.3 removes terminated jobs after the jobs builtin reports them.
        for job in jobs.iter().filter(|job| !job.running && !job.stopped) {
            let _ = self.host.disown_job(job.pid)?;
        }

        Ok(ExecutionResult::from_parts(stdout, String::new(), 0))
    }

    fn builtin_wait(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut wait_next = false;
        let mut assign_to: Option<String> = None;
        let mut targets = Vec::new();
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-n" => wait_next = true,
                "-f" => {}
                "-p" => {
                    index += 1;
                    assign_to = args.get(index).cloned();
                    if assign_to.is_none() {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "wait: -p requiere una variable\n".to_owned(),
                            2,
                        ));
                    }
                }
                "--" => {
                    targets.extend(args[index + 1..].iter().cloned());
                    break;
                }
                value if value.starts_with('-') => {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("wait: opción no válida: {value}\n"),
                        2,
                    ));
                }
                value => targets.push(value.to_owned()),
            }
            index += 1;
        }

        if wait_next {
            let Some((pid, status)) = self.host.wait_next_job()? else {
                return Ok(ExecutionResult::from_parts(String::new(), String::new(), 127));
            };
            if let Some(name) = assign_to {
                self.env.set(name, pid.to_string());
            }
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), status));
        }

        if targets.is_empty() {
            let status = self.host.wait_job(None)?;
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), status));
        }

        let mut status = 0;
        let mut stderr = String::new();
        for target in targets {
            let pid = if target.starts_with('%') {
                match self.resolve_jobspec(Some(&target))? {
                    Some((_, job)) => job.pid,
                    None => {
                        stderr.push_str(&format!("wait: {target}: no existe ese job\n"));
                        status = 127;
                        continue;
                    }
                }
            } else if let Ok(pid) = target.parse::<u32>() {
                pid
            } else {
                stderr.push_str(&format!("wait: {target}: identificador inválido\n"));
                status = 127;
                continue;
            };
            status = self.host.wait_job(Some(pid))?;
            if let Some(name) = assign_to.as_ref() {
                self.env.set(name.clone(), pid.to_string());
            }
        }
        Ok(ExecutionResult::from_parts(String::new(), stderr, status))
    }

    fn builtin_fg(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let spec = args.first().map(String::as_str);
        let Some((_, job)) = self.resolve_jobspec(spec)? else {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "fg: no hay job actual\n".to_owned(),
                1,
            ));
        };
        if job.stopped {
            if !self.host.signal_process(job.pid, "CONT")? {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("fg: %{}: no se pudo continuar el job\n", job.id),
                    1,
                ));
            }
        }
        let status = self.host.wait_job(Some(job.pid))?;
        Ok(ExecutionResult::from_parts(
            format!("{}\n", job.command),
            String::new(),
            status,
        ))
    }

    fn builtin_bg(&self, args: &[String]) -> Result<ExecutionResult> {
        let spec = args.first().map(String::as_str);
        let Some((number, job)) = self.resolve_jobspec(spec)? else {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "bg: no hay job actual\n".to_owned(),
                1,
            ));
        };

        if job.stopped {
            if !self.host.signal_process(job.pid, "CONT")? {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("bg: %{number}: no se pudo continuar el job\n"),
                    1,
                ));
            }
        } else if !job.running {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("bg: %{number}: el proceso ya terminó\n"),
                1,
            ));
        }

        Ok(ExecutionResult::from_parts(
            format!("[{number}] {} &\n", job.command),
            String::new(),
            0,
        ))
    }

    fn builtin_disown(&self, args: &[String]) -> Result<ExecutionResult> {
        let all = args.iter().any(|arg| arg == "-a");
        let running_only = args.iter().any(|arg| arg == "-r");
        let hup_only = args.iter().any(|arg| arg == "-h");
        let targets: Vec<&str> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(String::as_str)
            .collect();

        // Shell Shock Tool does not send SIGHUP to Windows child processes on exit,
        // so "disown -h" is already satisfied without removing the job.
        if hup_only {
            return Ok(ExecutionResult::success());
        }

        let jobs = self.host.jobs()?;
        let mut pids = Vec::new();
        if all || running_only {
            pids.extend(jobs.iter()
                .filter(|job| !running_only || job.running)
                .map(|job| job.pid));
        } else if targets.is_empty() {
            if let Some((_, job)) = self.resolve_jobspec(None)? {
                pids.push(job.pid);
            }
        } else {
            for target in targets {
                if let Some((_, job)) = self.resolve_jobspec(Some(target))? {
                    pids.push(job.pid);
                } else {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("disown: {target}: no existe ese job\n"),
                        1,
                    ));
                }
            }
        }

        for pid in pids {
            let _ = self.host.disown_job(pid)?;
        }
        Ok(ExecutionResult::success())
    }

    fn builtin_hash(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.is_empty() {
            let mut entries: Vec<_> = self.command_hash.iter().collect();
            entries.sort_by_key(|(name, _)| *name);
            let stdout = entries.into_iter()
                .map(|(_, path)| format!("0\t{path}\n"))
                .collect();
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        if args.iter().any(|arg| arg == "-r") {
            self.command_hash.clear();
            return Ok(ExecutionResult::success());
        }

        if args.first().map(String::as_str) == Some("-d") {
            for name in &args[1..] {
                self.command_hash.remove(name);
            }
            return Ok(ExecutionResult::success());
        }

        if args.first().map(String::as_str) == Some("-t") {
            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut status = 0;
            for name in &args[1..] {
                if let Some(path) = self.command_hash.get(name) {
                    stdout.push_str(path);
                    stdout.push('\n');
                } else {
                    stderr.push_str(&format!("hash: {name}: no encontrado\n"));
                    status = 1;
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, stderr, status));
        }

        if args.first().map(String::as_str) == Some("-p") {
            if args.len() < 3 {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    "hash: uso: hash -p ruta nombre\n".to_owned(),
                    2,
                ));
            }
            self.command_hash.insert(args[2].clone(), args[1].clone());
            return Ok(ExecutionResult::success());
        }

        let mut stderr = String::new();
        let mut status = 0;
        for name in args.iter().filter(|arg| !arg.starts_with('-')) {
            match self.host.execute_builtin("which", &[name.clone()], &self.env.cwd, None)? {
                Some(result) if result.status == 0 => {
                    if let Some(path) = result.stdout.lines().next().filter(|line| !line.is_empty()) {
                        self.command_hash.insert(name.clone(), path.to_owned());
                    } else {
                        status = 1;
                        stderr.push_str(&format!("hash: {name}: no encontrado\n"));
                    }
                }
                _ => {
                    status = 1;
                    stderr.push_str(&format!("hash: {name}: no encontrado\n"));
                }
            }
        }
        Ok(ExecutionResult::from_parts(String::new(), stderr, status))
    }


    fn builtin_exec(&mut self, args: &[String], stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        if self.env.option_enabled("restricted_shell") {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "exec: modo restringido: operación no permitida\n".to_owned(),
                1,
            ));
        }
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "-a" => index = (index + 2).min(args.len()),
                "-c" | "-l" => index += 1,
                _ => break,
            }
        }
        if index >= args.len() {
            self.persist_next_redirections = true;
            return Ok(ExecutionResult::success());
        }
        let mut result = self.execute_command_direct(&args[index], &args[index + 1..], stdin)?;
        let failed_to_exec = matches!(result.status, 126 | 127);
        if !failed_to_exec
            || (!self.env.option_enabled("interactive") && !self.env.option_enabled("execfail"))
        {
            result.exit_requested = true;
        }
        Ok(result)
    }

    fn history_path(&self) -> Option<PathBuf> {
        let path = self.env.get("HISTFILE");
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    fn history_entries(&self) -> Vec<HistoryEntry> {
        let Some(path) = self.history_path() else { return Vec::new(); };
        let Ok(text) = fs::read_to_string(path) else { return Vec::new(); };

        let mut entries = Vec::new();
        let mut pending_timestamp = None;
        for line in text.lines() {
            if let Some(raw) = line.strip_prefix('#')
                && !raw.is_empty()
                && raw.chars().all(|ch| ch.is_ascii_digit())
            {
                pending_timestamp = raw.parse::<i64>().ok();
                continue;
            }
            entries.push(HistoryEntry {
                timestamp: pending_timestamp.take(),
                command: line.to_owned(),
            });
        }
        entries
    }

    fn history_lines(&self) -> Vec<String> {
        self.history_entries()
            .into_iter()
            .map(|entry| entry.command)
            .collect()
    }

    fn save_history_entries(&self, entries: &[HistoryEntry]) -> Result<()> {
        let Some(path) = self.history_path() else { return Ok(()); };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut text = String::new();
        for entry in entries {
            if let Some(timestamp) = entry.timestamp {
                text.push('#');
                text.push_str(&timestamp.to_string());
                text.push('\n');
            }
            text.push_str(&entry.command);
            text.push('\n');
        }
        fs::write(path, text)?;
        Ok(())
    }

    fn save_history_lines(&self, lines: &[String]) -> Result<()> {
        let entries = lines.iter()
            .cloned()
            .map(|command| HistoryEntry { timestamp: None, command })
            .collect::<Vec<_>>();
        self.save_history_entries(&entries)
    }

    fn builtin_history(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut entries = self.history_entries();

        if args.first().map(String::as_str) == Some("-c") {
            entries.clear();
            self.save_history_entries(&entries)?;
            return Ok(ExecutionResult::success());
        }

        if args.first().map(String::as_str) == Some("-d") {
            let Some(position) = args.get(1).and_then(|value| value.parse::<usize>().ok()) else {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    "history: -d requiere posición\n".to_owned(),
                    2,
                ));
            };
            if position == 0 || position > entries.len() {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("history: {position}: posición inválida\n"),
                    1,
                ));
            }
            entries.remove(position - 1);
            self.save_history_entries(&entries)?;
            return Ok(ExecutionResult::success());
        }

        if args.first().map(String::as_str) == Some("-w") {
            self.save_history_entries(&entries)?;
            return Ok(ExecutionResult::success());
        }

        // The interactive engine records commands as they are accepted. For -a,
        // -r and -n the persistent file is already the authoritative history
        // source, so these operations are idempotent instead of duplicating rows.
        if matches!(args.first().map(String::as_str), Some("-a" | "-r" | "-n")) {
            return Ok(ExecutionResult::success());
        }

        let count = args.iter()
            .find_map(|value| value.parse::<usize>().ok())
            .unwrap_or(entries.len());
        let start = entries.len().saturating_sub(count);
        let time_format = self.env.get("HISTTIMEFORMAT");
        let mut stdout = String::new();

        for (index, entry) in entries.iter().enumerate().skip(start) {
            let rendered_time = if time_format.is_empty() {
                String::new()
            } else if let Some(timestamp) = entry.timestamp {
                use chrono::TimeZone;
                chrono::Local.timestamp_opt(timestamp, 0)
                    .single()
                    .map(|value| value.format(&time_format).to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            stdout.push_str(&format!(
                "{:5}  {}{}\n",
                index + 1,
                rendered_time,
                entry.command
            ));
        }
        Ok(ExecutionResult::from_parts(stdout, String::new(), 0))
    }

    fn builtin_fc(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let history = self.history_lines();
        if history.is_empty() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "fc: historial vacío\n".to_owned(),
                1,
            ));
        }

        let mut list = false;
        let mut substitute = false;
        let mut reverse = false;
        let mut number_lines = true;
        let mut editor: Option<String> = None;
        let mut operands = Vec::new();
        let mut index = 0usize;

        while index < args.len() {
            let arg = &args[index];
            if arg == "--" {
                operands.extend(args[index + 1..].iter().cloned());
                break;
            }
            if arg == "-e" {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        "fc: -e requiere editor\n".to_owned(),
                        2,
                    ));
                };
                editor = Some(value.clone());
            } else if arg.starts_with('-')
                && arg.len() > 1
                && !arg[1..].chars().all(|ch| ch.is_ascii_digit())
            {
                for flag in arg[1..].chars() {
                    match flag {
                        'l' => list = true,
                        'n' => number_lines = false,
                        'r' => reverse = true,
                        's' => substitute = true,
                        _ => return Ok(ExecutionResult::from_parts(
                            String::new(),
                            format!("fc: -{flag}: opción inválida\n"),
                            2,
                        )),
                    }
                }
            } else {
                operands.push(arg.clone());
            }
            index += 1;
        }

        let resolve = |value: Option<&String>, default: usize| -> usize {
            let Some(value) = value else { return default; };
            if let Ok(number) = value.parse::<isize>() {
                if number < 0 {
                    return history.len()
                        .saturating_sub((-number) as usize)
                        .saturating_add(1)
                        .clamp(1, history.len());
                }
                return (number as usize).clamp(1, history.len());
            }
            history.iter()
                .rposition(|line| line.starts_with(value))
                .map(|index| index + 1)
                .unwrap_or(default)
        };

        if substitute {
            let mut substitution = None;
            let mut selector = None;
            for operand in &operands {
                if substitution.is_none() && operand.contains('=') {
                    substitution = operand.split_once('=')
                        .map(|(from, to)| (from.to_owned(), to.to_owned()));
                } else if selector.is_none() {
                    selector = Some(operand);
                }
            }
            let position = resolve(selector, history.len());
            let mut command = history.get(position.saturating_sub(1)).cloned().unwrap_or_default();
            if let Some((from, to)) = substitution {
                command = command.replacen(&from, &to, 1);
            }
            return self.execute_text(&command);
        }

        let default_first = if list {
            history.len().saturating_sub(15).max(1)
        } else {
            history.len()
        };
        let first = resolve(operands.first(), default_first);
        let last = resolve(operands.get(1), if list { history.len() } else { first });

        let mut rows: Vec<(usize, String)> = (first.min(last)..=first.max(last))
            .filter_map(|number| history.get(number - 1).cloned().map(|line| (number, line)))
            .collect();
        if reverse || first > last {
            rows.reverse();
        }

        if list {
            let stdout = rows.into_iter()
                .map(|(number, line)| {
                    if number_lines { format!("{number}\t{line}\n") } else { format!("{line}\n") }
                })
                .collect();
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        let mut source = rows.into_iter().map(|(_, line)| line).collect::<Vec<_>>().join("\n");
        let selected_editor = editor
            .or_else(|| {
                let value = self.env.get("FCEDIT");
                (!value.is_empty()).then_some(value)
            })
            .or_else(|| {
                let value = self.env.get("EDITOR");
                (!value.is_empty()).then_some(value)
            })
            .unwrap_or_else(|| "vi".to_owned());

        if selected_editor != "-" {
            let path = std::env::temp_dir().join(format!(
                "sst-fc-{}-{}.sh",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            ));
            fs::write(&path, &source)?;
            let editor_words = split_shell_words_relaxed(&selected_editor)
                .unwrap_or_else(|_| vec![selected_editor.clone()]);
            let Some((program, editor_args)) = editor_words.split_first() else {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    "fc: editor vacío\n".to_owned(),
                    1,
                ));
            };
            let mut edit_args = editor_args.to_vec();
            edit_args.push(path.to_string_lossy().into_owned());
            let edit_result = self.execute_command_direct(program, &edit_args, None)?;
            if edit_result.status != 0 {
                let _ = fs::remove_file(&path);
                return Ok(edit_result);
            }
            source = fs::read_to_string(&path)?;
            let _ = fs::remove_file(&path);
        }

        let mut result = self.execute_text(&source)?;
        result.stdout = format!("{source}\n{}", result.stdout);
        Ok(result)
    }

    fn builtin_bind(&mut self, args: &[String]) -> Result<ExecutionResult> {
        const FUNCTIONS: &[&str] = &[
            "beginning-of-line", "end-of-line", "forward-char", "backward-char",
            "delete-char", "backward-delete-char", "previous-history", "next-history",
            "complete", "clear-screen", "unix-line-discard", "quoted-insert",
            "bash-vi-complete", "execute-named-command", "export-completions",
        ];

        if args.first().map(String::as_str) == Some("-l") {
            return Ok(ExecutionResult::from_parts(
                format!("{}\n", FUNCTIONS.join("\n")),
                String::new(),
                0,
            ));
        }

        if args.first().map(String::as_str) == Some("-q") {
            let Some(function) = args.get(1) else {
                return Ok(ExecutionResult::from_parts(String::new(), "bind: -q requiere función\n".to_owned(), 2));
            };
            let mut stdout = String::new();
            let mut found = false;
            for (key, value) in &self.readline_bindings {
                if value.trim() == function {
                    stdout.push_str(&format!("{function} puede invocarse mediante \"{key}\"\n"));
                    found = true;
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), if found { 0 } else { 1 }));
        }

        if args.first().map(String::as_str) == Some("-u") {
            let Some(function) = args.get(1) else {
                return Ok(ExecutionResult::from_parts(String::new(), "bind: -u requiere función\n".to_owned(), 2));
            };
            self.readline_bindings.retain(|_, value| value.trim() != function);
            return Ok(ExecutionResult::success());
        }

        if args.first().map(String::as_str) == Some("-f") {
            let Some(path) = args.get(1) else {
                return Ok(ExecutionResult::from_parts(String::new(), "bind: -f requiere archivo\n".to_owned(), 2));
            };
            let content = match fs::read_to_string(self.resolve_path(path)) {
                Ok(content) => content,
                Err(error) => return Ok(ExecutionResult::from_parts(
                    String::new(), format!("bind: {path}: {error}\n"), 1,
                )),
            };
            for line in content.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
                if let Some((key, command)) = line.split_once(':') {
                    self.readline_bindings.insert(strip_outer_quotes(key.trim()), command.trim().to_owned());
                }
            }
            return Ok(ExecutionResult::success());
        }

        if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-p" | "-P" | "-s" | "-S" | "-v" | "-V")) {
            let filter_names: Vec<&str> = args.iter()
                .skip_while(|arg| arg.starts_with('-'))
                .map(String::as_str)
                .collect();
            let mut entries: Vec<_> = self.readline_bindings.iter()
                .filter(|(_, value)| filter_names.is_empty() || filter_names.iter().any(|name| value.trim() == *name))
                .collect();
            entries.sort_by_key(|(key, _)| *key);
            let stdout = entries.into_iter()
                .map(|(key, value)| format!("\"{}\": {}\n", key.replace('"', "\\\""), value))
                .collect();
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        if args.first().map(String::as_str) == Some("-r") {
            if let Some(key) = args.get(1) { self.readline_bindings.remove(key); }
            return Ok(ExecutionResult::success());
        }

        let mut offset = 0usize;
        if args.first().map(String::as_str) == Some("-m") {
            // Shell Shock Tool uses one native editing map; accept Bash keymap
            // selection so inputrc-compatible scripts do not fail.
            offset = 2.min(args.len());
        }
        if args.get(offset).map(String::as_str) == Some("-x") { offset += 1; }

        let remaining = &args[offset..];
        let mut index = 0usize;
        while index < remaining.len() {
            let binding = &remaining[index];
            if let Some((key, command)) = binding.split_once(':') {
                let command = strip_outer_quotes(command.trim());
                self.readline_bindings.insert(
                    strip_outer_quotes(key.trim()),
                    if args.iter().any(|arg| arg == "-x") { format!("shell:{command}") } else { command },
                );
                index += 1;
                continue;
            }

            if args.iter().any(|arg| arg == "-x") && index + 1 < remaining.len() {
                let key = strip_outer_quotes(binding.trim());
                let command = strip_outer_quotes(remaining[index + 1].trim());
                self.readline_bindings.insert(key, format!("shell:{command}"));
                index += 2;
                continue;
            }
            index += 1;
        }
        Ok(ExecutionResult::success())
    }

    fn builtin_enable(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let disable = args.iter().any(|arg| arg == "-n");
        let special_only = args.iter().any(|arg| arg == "-s");
        let print_all = args.iter().any(|arg| arg == "-a");
        let print_enabled = args.iter().any(|arg| arg == "-p");

        if args.iter().any(|arg| matches!(arg.as_str(), "-f" | "-d")) {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "enable: los builtins cargables dinámicamente no están disponibles en esta compilación nativa\n".to_owned(),
                1,
            ));
        }

        let special = bash_special_builtin_names();
        if args.is_empty() || print_all || print_enabled {
            let mut names: Vec<_> = bash_builtin_names().iter()
                .copied()
                .filter(|name| !special_only || special.contains(name))
                .collect();
            names.sort();

            let mut stdout = String::new();
            for name in names {
                let disabled = self.disabled_builtins.contains(name);
                if print_enabled && disabled { continue; }
                stdout.push_str(if disabled { "enable -n " } else { "enable " });
                stdout.push_str(name);
                stdout.push('\n');
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        for name in args.iter().filter(|arg| !arg.starts_with('-')) {
            if !bash_builtin_names().contains(&name.as_str())
                || (special_only && !special.contains(&name.as_str()))
            {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("enable: {name}: no es builtin{}\n", if special_only { " especial" } else { "" }),
                    1,
                ));
            }
            if disable { self.disabled_builtins.insert(name.clone()); }
            else { self.disabled_builtins.remove(name); }
        }
        Ok(ExecutionResult::success())
    }

    fn parse_completion_spec(&self, args: &[String]) -> Result<(CompletionSpec, Vec<String>)> {
        let mut spec = CompletionSpec::default();
        let mut names = Vec::new();
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "-W" => {
                    index += 1;
                    spec.words = args.get(index)
                        .map(|value| split_shell_words_relaxed(value).unwrap_or_default())
                        .unwrap_or_default();
                }
                "-A" => { index += 1; spec.action = args.get(index).cloned(); }
                "-F" => { index += 1; spec.function = args.get(index).cloned(); }
                "-C" => { index += 1; spec.command = args.get(index).cloned(); }
                "-P" => { index += 1; spec.prefix = args.get(index).cloned().unwrap_or_default(); }
                "-S" => { index += 1; spec.suffix = args.get(index).cloned().unwrap_or_default(); }
                "-o" => {
                    index += 1;
                    if let Some(value) = args.get(index) { spec.options.insert(value.clone()); }
                }
                "-a" => spec.action = Some("alias".to_owned()),
                "-b" => spec.action = Some("builtin".to_owned()),
                "-c" => spec.action = Some("command".to_owned()),
                "-d" => spec.action = Some("directory".to_owned()),
                "-e" => spec.action = Some("export".to_owned()),
                "-f" => spec.action = Some("file".to_owned()),
                "-g" => spec.action = Some("group".to_owned()),
                "-j" => spec.action = Some("job".to_owned()),
                "-k" => spec.action = Some("keyword".to_owned()),
                "-s" => spec.action = Some("service".to_owned()),
                "-u" => spec.action = Some("user".to_owned()),
                "-v" => spec.action = Some("variable".to_owned()),
                "--" => {
                    names.extend(args[index + 1..].iter().cloned());
                    break;
                }
                value if value.starts_with('-') => {}
                value => names.push(value.to_owned()),
            }
            index += 1;
        }
        Ok((spec, names))
    }

    fn builtin_complete(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.iter().any(|arg| arg == "-r") {
            let names: Vec<_> = args.iter()
                .filter(|arg| !arg.starts_with('-'))
                .cloned()
                .collect();
            if names.is_empty() {
                self.completion_specs.clear();
            } else {
                for name in names {
                    self.completion_specs.remove(&name);
                }
            }
            return Ok(ExecutionResult::success());
        }

        if args.is_empty() || args.iter().any(|arg| arg == "-p") {
            let mut rows: Vec<_> = self.completion_specs.iter().collect();
            rows.sort_by_key(|(name, _)| *name);
            let stdout = rows.into_iter()
                .map(|(name, spec)| {
                    let mut parts = vec!["complete".to_owned()];
                    if let Some(action) = &spec.action {
                        parts.extend(["-A".to_owned(), shell_quote(action)]);
                    }
                    if !spec.words.is_empty() {
                        parts.extend(["-W".to_owned(), shell_quote(&spec.words.join(" "))]);
                    }
                    if let Some(function) = &spec.function {
                        parts.extend(["-F".to_owned(), shell_quote(function)]);
                    }
                    if let Some(command) = &spec.command {
                        parts.extend(["-C".to_owned(), shell_quote(command)]);
                    }
                    if !spec.prefix.is_empty() {
                        parts.extend(["-P".to_owned(), shell_quote(&spec.prefix)]);
                    }
                    if !spec.suffix.is_empty() {
                        parts.extend(["-S".to_owned(), shell_quote(&spec.suffix)]);
                    }
                    parts.push(shell_quote(name));
                    format!("{}\n", parts.join(" "))
                })
                .collect();
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        let (spec, names) = self.parse_completion_spec(args)?;
        if names.is_empty() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "complete: falta nombre de comando\n".to_owned(),
                2,
            ));
        }
        for name in names {
            self.completion_specs.insert(name, spec.clone());
        }
        Ok(ExecutionResult::success())
    }

    fn completion_candidates_for_action(&self, action: &str, prefix: &str) -> Vec<String> {
        let mut values: Vec<String> = match action {
            "alias" => self.env.aliases.keys().cloned().collect(),
            "arrayvar" => self.env.arrays.keys().chain(self.env.assoc_arrays.keys()).cloned().collect(),
            "binding" => self.readline_bindings.keys().cloned().collect(),
            "builtin" => bash_builtin_names().iter().map(|name| (*name).to_owned()).collect(),
            "disabled" => self.disabled_builtins.iter().cloned().collect(),
            "enabled" => bash_builtin_names().iter()
                .filter(|name| !self.disabled_builtins.contains(**name))
                .map(|name| (*name).to_owned())
                .collect(),
            "export" => self.env.exported.keys().cloned().collect(),
            "function" => self.env.functions.keys().cloned().collect(),
            "helptopic" => bash_builtin_names().iter().map(|name| (*name).to_owned()).collect(),
            "variable" => self.env.vars.keys()
                .chain(self.env.arrays.keys())
                .chain(self.env.assoc_arrays.keys())
                .chain(self.env.namerefs.keys())
                .cloned()
                .collect(),
            "keyword" => bash_keywords().iter().map(|word| (*word).to_owned()).collect(),
            "job" => self.host.jobs().unwrap_or_default().into_iter()
                .map(|job| format!("%{}", job.id))
                .collect(),
            "running" => self.host.jobs().unwrap_or_default().into_iter()
                .filter(|job| job.running)
                .map(|job| format!("%{}", job.id))
                .collect(),
            "stopped" => self.host.jobs().unwrap_or_default().into_iter()
                .filter(|job| job.stopped)
                .map(|job| format!("%{}", job.id))
                .collect(),
            "signal" => bash_signal_names().iter().map(|name| (*name).to_owned()).collect(),
            "setopt" => bash_shell_options().iter().map(|name| (*name).to_owned()).collect(),
            "shopt" => bash_shopt_options().iter().map(|name| (*name).to_owned()).collect(),
            "user" => self.host.user_names(),
            "group" => self.host.group_names(),
            "service" => self.host.command_names().into_iter()
                .filter(|name| name.eq_ignore_ascii_case("sc") || name.eq_ignore_ascii_case("net"))
                .collect(),
            "hostname" => std::env::var("COMPUTERNAME").ok().into_iter().collect(),
            "directory" => self.path_completions(prefix, true),
            "file" => self.path_completions(prefix, false),
            "command" => {
                let mut rows: Vec<String> = bash_builtin_names().iter()
                    .map(|name| (*name).to_owned())
                    .collect();
                rows.extend(self.env.functions.keys().cloned());
                rows.extend(self.env.aliases.keys().cloned());
                rows.extend(self.host.command_names());
                rows.extend(path_commands(&self.env.get("PATH")));
                rows
            }
            _ => Vec::new(),
        };
        values.retain(|value| value.starts_with(prefix));
        values.sort();
        values.dedup();
        values
    }

    fn path_completions(&self, prefix: &str, directories_only: bool) -> Vec<String> {
        let mut values = completion_files(&self.env.cwd, prefix, directories_only);

        if values.is_empty() && self.env.option_enabled("dirspell") {
            let typed = PathBuf::from(prefix);
            let stem = typed.file_name().and_then(|name| name.to_str()).unwrap_or("");
            if let Some(parent) = typed.parent().filter(|path| !path.as_os_str().is_empty()) {
                if let Some(corrected) = self.correct_directory_spelling(&parent.to_string_lossy()) {
                    let corrected_prefix = corrected.join(stem).to_string_lossy().into_owned();
                    values = completion_files(&self.env.cwd, &corrected_prefix, directories_only);
                }
            }
        }

        if self.env.option_enabled("direxpand") {
            values = values.into_iter().map(|value| {
                let trailing = value.ends_with('/') || value.ends_with('\\');
                let path = self.resolve_path(&value);
                let mut rendered = fs::canonicalize(&path)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                if trailing && !rendered.ends_with(std::path::MAIN_SEPARATOR) {
                    rendered.push(std::path::MAIN_SEPARATOR);
                }
                rendered
            }).collect();
        }

        values.sort();
        values.dedup();
        values
    }

    fn apply_completion_filters(&self, mut values: Vec<String>) -> Vec<String> {
        let original = values.clone();

        let fignore = self.env.get("FIGNORE");
        if !fignore.is_empty() {
            let suffixes: Vec<&str> = fignore.split(':')
                .filter(|suffix| !suffix.is_empty())
                .collect();
            values.retain(|value| !suffixes.iter().any(|suffix| value.ends_with(suffix)));
            if values.is_empty() && !self.env.option_enabled("force_fignore") {
                values = original;
            }
        }

        values
    }

    fn generate_completions(
        &mut self,
        spec: &CompletionSpec,
        prefix: &str,
        line: &str,
    ) -> Result<Vec<String>> {
        let mut values = spec.words.clone();

        if let Some(action) = &spec.action {
            values.extend(self.completion_candidates_for_action(action, prefix));
        }

        if let Some(function) = &spec.function {
            if let Some(body) = self.env.functions.get(function).cloned() {
                let saved = self.env.positional.clone();
                self.env.positional = vec![line.to_owned(), prefix.to_owned(), String::new()];
                self.env.set_array("COMPREPLY", Vec::new());
                let _ = self.execute(&body, None)?;
                values.extend(self.env.array_values("COMPREPLY"));
                self.env.positional = saved;
            }
        }

        if let Some(command) = &spec.command {
            let result = self.execute_text(command)?;
            values.extend(result.stdout.lines().map(str::to_owned));
        }

        if self.env.option_enabled("nocasematch") {
            let needle = prefix.to_lowercase();
            values.retain(|value| value.to_lowercase().starts_with(&needle));
        } else {
            values.retain(|value| value.starts_with(prefix));
        }
        values = self.apply_completion_filters(values);
        let fullquote = spec.options.contains("fullquote")
            || self.env.option_enabled("complete_fullquote");
        values = values.into_iter()
            .map(|value| {
                let value = format!("{}{}{}", spec.prefix, value, spec.suffix);
                if fullquote { shell_quote(&value) } else { value }
            })
            .collect();
        values.sort();
        values.dedup();
        Ok(values)
    }

    fn builtin_compgen(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut array_target = None;
        let mut filtered = Vec::new();
        let mut index = 0usize;
        while index < args.len() {
            if args[index] == "-V" {
                index += 1;
                let Some(name) = args.get(index) else {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        "compgen: -V requiere nombre de array\n".to_owned(),
                        2,
                    ));
                };
                array_target = Some(name.clone());
            } else {
                filtered.push(args[index].clone());
            }
            index += 1;
        }

        let (spec, names) = self.parse_completion_spec(&filtered)?;
        let prefix = names.last().cloned().unwrap_or_default();
        let values = self.generate_completions(&spec, &prefix, &prefix)?;
        let status = if values.is_empty() { 1 } else { 0 };

        if let Some(name) = array_target {
            self.env.set_array(name, values);
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), status));
        }

        let stdout = if values.is_empty() {
            String::new()
        } else {
            format!("{}\n", values.join("\n"))
        };
        Ok(ExecutionResult::from_parts(stdout, String::new(), status))
    }

    fn builtin_compopt(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut add = Vec::new();
        let mut remove = Vec::new();
        let mut names = Vec::new();
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-o" => {
                    index += 1;
                    if let Some(value) = args.get(index) { add.push(value.clone()); }
                }
                "+o" => {
                    index += 1;
                    if let Some(value) = args.get(index) { remove.push(value.clone()); }
                }
                value if value.starts_with('-') => {}
                value => names.push(value.to_owned()),
            }
            index += 1;
        }

        if names.is_empty() {
            let mut stdout = String::new();
            for (name, spec) in &self.completion_specs {
                for option in &spec.options {
                    stdout.push_str(&format!("compopt -o {option} {name}\n"));
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        for name in names {
            let spec = self.completion_specs.entry(name).or_default();
            for option in &add { spec.options.insert(option.clone()); }
            for option in &remove { spec.options.remove(option); }
        }
        Ok(ExecutionResult::success())
    }

    fn builtin_suspend(&self, args: &[String]) -> Result<ExecutionResult> {
        if args.is_empty() || args.iter().any(|arg| arg == "-f") {
            let _ = self.host.read_line("Shell suspendida. Pulse Enter para continuar.", true)?;
            return Ok(ExecutionResult::success());
        }
        Ok(ExecutionResult::from_parts(
            String::new(),
            "suspend: opción inválida\n".to_owned(),
            2,
        ))
    }

    fn correct_directory_spelling(&self, raw: &str) -> Option<PathBuf> {
        let requested = PathBuf::from(raw);
        let name = requested.file_name()?.to_string_lossy();
        let parent_raw = requested.parent().filter(|path| !path.as_os_str().is_empty());
        let parent = parent_raw
            .map(|path| self.resolve_path(&path.to_string_lossy()))
            .unwrap_or_else(|| self.env.cwd.clone());

        let mut matches = fs::read_dir(&parent).ok()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| {
                let candidate = entry.file_name().to_string_lossy().into_owned();
                spelling_distance_one(&name, &candidate).then(|| entry.path())
            })
            .collect::<Vec<_>>();
        matches.sort();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn install_directory_stack(&mut self, stack: Vec<PathBuf>) -> Result<()> {
        let Some(target) = stack.first().cloned() else { return Ok(()); };
        let previous = self.env.cwd.clone();
        self.env.cwd = fs::canonicalize(&target).unwrap_or(target);
        self.env.oldpwd = Some(previous.clone());
        self.env.set("OLDPWD", previous.to_string_lossy().into_owned());
        self.env.set("PWD", self.env.cwd.to_string_lossy().into_owned());
        self.env.dir_stack = stack.into_iter().skip(1).rev().collect();
        Ok(())
    }

    fn builtin_dirs(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.iter().any(|arg| arg == "-c") {
            self.env.dir_stack.clear();
            return Ok(ExecutionResult::success());
        }

        let stack = self.directory_stack();
        let selector = args.iter().find(|arg| {
            arg.starts_with('+') || (arg.starts_with('-')
                && arg.len() > 1 && arg[1..].chars().all(|ch| ch.is_ascii_digit()))
        });
        let selected = selector.and_then(|arg| {
            let number = arg[1..].parse::<usize>().ok()?;
            if arg.starts_with('+') {
                stack.get(number)
            } else {
                stack.len().checked_sub(number + 1).and_then(|index| stack.get(index))
            }
        });

        if selector.is_some() && selected.is_none() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "dirs: índice fuera de rango\n".to_owned(),
                1,
            ));
        }

        let paths: Vec<&PathBuf> = selected.map(|path| vec![path]).unwrap_or_else(|| stack.iter().collect());
        let stdout = if args.iter().any(|arg| arg == "-v") {
            paths.iter().enumerate()
                .map(|(index, path)| format!("{index}  {}\n", path.display()))
                .collect()
        } else if args.iter().any(|arg| arg == "-p") {
            paths.iter().map(|path| format!("{}\n", path.display())).collect()
        } else {
            format!(
                "{}\n",
                paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>().join(" ")
            )
        };
        Ok(ExecutionResult::from_parts(stdout, String::new(), 0))
    }

    fn builtin_pushd(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if self.env.option_enabled("restricted_shell") {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "pushd: modo restringido: operación no permitida\n".to_owned(),
                1,
            ));
        }

        let no_cd = args.iter().any(|arg| arg == "-n");
        let operand = args.iter().find(|arg| arg.as_str() != "-n").map(String::as_str);
        let mut stack = self.directory_stack();

        match operand {
            None => {
                if stack.len() < 2 {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        "pushd: no hay otro directorio\n".to_owned(),
                        1,
                    ));
                }
                stack.swap(0, 1);
            }
            Some(value) if value.starts_with('+') || (value.starts_with('-')
                && value.len() > 1 && value[1..].chars().all(|ch| ch.is_ascii_digit())) =>
            {
                let number = value[1..].parse::<usize>().unwrap_or(usize::MAX);
                if number >= stack.len() {
                    return Ok(ExecutionResult::from_parts(
                        String::new(), "pushd: índice fuera de rango\n".to_owned(), 1,
                    ));
                }
                let rotate = if value.starts_with('+') {
                    number
                } else {
                    stack.len() - number - 1
                };
                stack.rotate_left(rotate);
            }
            Some(value) => {
                let target = self.resolve_path(value);
                if !target.is_dir() {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("pushd: {value}: directorio inválido\n"),
                        1,
                    ));
                }
                let target = fs::canonicalize(target).unwrap_or_else(|_| self.resolve_path(value));
                if no_cd {
                    stack.insert(1.min(stack.len()), target);
                } else {
                    stack.insert(0, target);
                }
            }
        }

        if !no_cd {
            self.install_directory_stack(stack)?;
        } else {
            self.env.dir_stack = stack.into_iter().skip(1).rev().collect();
        }
        self.builtin_dirs(&[])
    }

    fn builtin_popd(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if self.env.option_enabled("restricted_shell") {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "popd: modo restringido: operación no permitida\n".to_owned(),
                1,
            ));
        }

        let no_cd = args.iter().any(|arg| arg == "-n");
        let operand = args.iter().find(|arg| arg.as_str() != "-n").map(String::as_str);
        let mut stack = self.directory_stack();
        if stack.len() <= 1 {
            return Ok(ExecutionResult::from_parts(
                String::new(), "popd: pila vacía\n".to_owned(), 1,
            ));
        }

        let index = match operand {
            None => 0,
            Some(value) if value.starts_with('+') => value[1..].parse::<usize>().unwrap_or(usize::MAX),
            Some(value) if value.starts_with('-') => {
                let number = value[1..].parse::<usize>().unwrap_or(usize::MAX);
                stack.len().checked_sub(number + 1).unwrap_or(usize::MAX)
            }
            Some(value) => {
                return Ok(ExecutionResult::from_parts(
                    String::new(), format!("popd: opción inválida: {value}\n"), 2,
                ));
            }
        };

        if index >= stack.len() {
            return Ok(ExecutionResult::from_parts(
                String::new(), "popd: índice fuera de rango\n".to_owned(), 1,
            ));
        }
        stack.remove(index);

        if !no_cd && index == 0 {
            self.install_directory_stack(stack)?;
        } else {
            let current = stack.remove(0);
            self.env.cwd = current;
            self.env.dir_stack = stack.into_iter().rev().collect();
        }
        self.builtin_dirs(&[])
    }

    fn builtin_umask(&mut self, args: &[String]) -> ExecutionResult {
        let symbolic = args.iter().any(|arg| arg == "-S");
        let reusable = args.iter().any(|arg| arg == "-p");
        let operand = args.iter().find(|arg| !arg.starts_with('-'));

        if let Some(value) = operand {
            let parsed = if value.chars().all(|ch| matches!(ch, '0'..='7')) {
                u16::from_str_radix(value.trim_start_matches('0').if_empty("0"), 8).ok()
            } else {
                parse_symbolic_umask(value)
            };
            let Some(mask) = parsed.filter(|value| *value <= 0o777) else {
                return ExecutionResult::from_parts(
                    String::new(),
                    format!("umask: {value}: máscara inválida\n"),
                    1,
                );
            };
            self.env.set("__UMASK", format!("{mask:04o}"));
            return ExecutionResult::success();
        }

        let mask = u16::from_str_radix(
            self.env.get("__UMASK").trim_start_matches('0').if_empty("0"),
            8,
        ).unwrap_or(0o022);

        let text = if symbolic {
            let perms = 0o777u16 & !mask;
            let render = |shift: u16| {
                let bits = (perms >> shift) & 0o7;
                format!(
                    "{}{}{}",
                    if bits & 0o4 != 0 { "r" } else { "" },
                    if bits & 0o2 != 0 { "w" } else { "" },
                    if bits & 0o1 != 0 { "x" } else { "" },
                )
            };
            format!("u={},g={},o={}", render(6), render(3), render(0))
        } else {
            format!("{mask:04o}")
        };
        let stdout = if reusable { format!("umask {text}\n") } else { format!("{text}\n") };
        ExecutionResult::from_parts(stdout, String::new(), 0)
    }

    fn builtin_ulimit(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let labels: &[(char, &str)] = &[
            ('b', "socket buffer size"), ('c', "core file size"), ('d', "data seg size"),
            ('e', "scheduling priority"), ('f', "file size"), ('i', "pending signals"),
            ('k', "kqueues"), ('l', "max locked memory"), ('m', "max memory size"),
            ('n', "open files"), ('p', "pipe size"), ('q', "POSIX message queues"),
            ('r', "real-time priority"), ('s', "stack size"), ('t', "cpu time"),
            ('u', "max user processes"), ('v', "virtual memory"), ('x', "file locks"),
            ('P', "pseudoterminals"), ('R', "real-time non-blocking time"),
            ('T', "max threads"),
        ];

        if args.iter().any(|arg| arg == "-a") {
            let mut stdout = String::new();
            for (flag, label) in labels {
                let value = self.ulimits.get(flag)
                    .cloned()
                    .unwrap_or_else(|| "unlimited".to_owned());
                stdout.push_str(&format!("{label} (-{flag}) {value}\n"));
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        let mut selected = 'f';
        let mut value = None;
        for arg in args {
            if matches!(arg.as_str(), "-S" | "-H") {
                continue;
            }
            if arg.starts_with('-') && arg.len() == 2 {
                selected = arg.chars().nth(1).unwrap_or('f');
            } else {
                value = Some(arg.clone());
            }
        }

        if !labels.iter().any(|(flag, _)| *flag == selected) {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("ulimit: -{selected}: opción inválida\n"),
                2,
            ));
        }

        if let Some(value) = value {
            if value != "unlimited" && value.parse::<u64>().is_err() {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("ulimit: {value}: límite inválido\n"),
                    2,
                ));
            }
            self.ulimits.insert(selected, value);
            Ok(ExecutionResult::success())
        } else {
            Ok(ExecutionResult::from_parts(
                format!(
                    "{}\n",
                    self.ulimits.get(&selected)
                        .cloned()
                        .unwrap_or_else(|| "unlimited".to_owned())
                ),
                String::new(),
                0,
            ))
        }
    }

    fn builtin_times(&self) -> ExecutionResult {
        let (user, system, child_user, child_system) =
            self.host.process_times().unwrap_or((0.0, 0.0, 0.0, 0.0));
        ExecutionResult::from_parts(
            format!(
                "{} {}\n{} {}\n",
                format_shell_cpu_time(user),
                format_shell_cpu_time(system),
                format_shell_cpu_time(child_user),
                format_shell_cpu_time(child_system),
            ),
            String::new(),
            0,
        )
    }

    fn builtin_caller(&self, args: &[String]) -> ExecutionResult {
        let index = args.first().and_then(|arg| arg.parse::<usize>().ok()).unwrap_or(0);
        let Some(frame) = self.call_stack.iter().rev().nth(index) else {
            return ExecutionResult::from_parts(String::new(), String::new(), 1);
        };
        ExecutionResult::from_parts(
            format!("{} {} {}\n", frame.line, frame.function, frame.source),
            String::new(),
            0,
        )
    }

    fn builtin_echo(&self, args: &[String]) -> ExecutionResult {
        let mut newline = true;
        let mut escapes = self.env.option_enabled("xpg_echo");
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-n" => newline = false,
                "-e" => escapes = true,
                "-E" => escapes = false,
                value if value.starts_with('-')
                    && value.len() > 1
                    && value[1..].chars().all(|ch| matches!(ch, 'n' | 'e' | 'E')) =>
                {
                    for ch in value[1..].chars() {
                        match ch {
                            'n' => newline = false,
                            'e' => escapes = true,
                            'E' => escapes = false,
                            _ => {}
                        }
                    }
                }
                _ => break,
            }
            index += 1;
        }

        let text = args[index..].join(" ");
        let (mut stdout, stop) = if escapes { decode_backslash_escapes(&text, true) } else { (text, false) };
        if newline && !stop { stdout.push('\n'); }
        ExecutionResult::from_parts(stdout, String::new(), 0)
    }

    fn builtin_printf(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut args = args;
        let mut assign_to: Option<String> = None;
        if args.first().map(String::as_str) == Some("-v") {
            let Some(name) = args.get(1) else {
                return Ok(ExecutionResult::from_parts(
                    String::new(),
                    "printf: -v requiere variable\n".to_owned(),
                    2,
                ));
            };
            assign_to = Some(name.clone());
            args = &args[2..];
        }

        let Some(raw_format) = args.first() else {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "printf: falta formato\n".to_owned(),
                2,
            ));
        };
        let values = &args[1..];

        let (format, format_stops) = decode_backslash_escapes(raw_format, true);
        let chars: Vec<char> = format.chars().collect();
        let mut output = String::new();
        let mut value_index = 0usize;
        let mut first_pass = true;
        let mut stop_all = false;

        while first_pass || (!stop_all && value_index < values.len()) {
            first_pass = false;
            let pass_start = value_index;
            let mut i = 0usize;

            while i < chars.len() && !stop_all {
                if chars[i] != '%' {
                    output.push(chars[i]);
                    i += 1;
                    continue;
                }
                if chars.get(i + 1) == Some(&'%') {
                    output.push('%');
                    i += 2;
                    continue;
                }

                i += 1;

                if chars.get(i) == Some(&'(') {
                    let mut close = i + 1;
                    while close < chars.len() && chars[close] != ')' { close += 1; }
                    if close < chars.len() && chars.get(close + 1) == Some(&'T') {
                        let date_format: String = chars[i + 1..close].iter().collect();
                        let value = values.get(value_index).cloned().unwrap_or_else(|| "-1".to_owned());
                        value_index += 1;
                        let timestamp = parse_printf_integer(&value).unwrap_or(-1);
                        let rendered = if timestamp == -1 {
                            chrono::Local::now().format(&date_format).to_string()
                        } else if timestamp == -2 {
                            chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH)
                                .format(&date_format)
                                .to_string()
                        } else if let Some(utc) = chrono::DateTime::from_timestamp(timestamp, 0) {
                            utc.with_timezone(&chrono::Local).format(&date_format).to_string()
                        } else {
                            String::new()
                        };
                        output.push_str(&rendered);
                        i = close + 2;
                        continue;
                    }
                }

                let mut left = false;
                let mut plus = false;
                let mut space = false;
                let mut alternate = false;
                let mut zero = false;
                while i < chars.len() {
                    match chars[i] {
                        '-' => left = true,
                        '+' => plus = true,
                        ' ' => space = true,
                        '#' => alternate = true,
                        '0' => zero = true,
                        _ => break,
                    }
                    i += 1;
                }

                let width = if chars.get(i) == Some(&'*') {
                    i += 1;
                    let raw = values.get(value_index).cloned().unwrap_or_default();
                    value_index += 1;
                    raw.parse::<isize>().unwrap_or(0)
                } else {
                    let begin = i;
                    while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                    chars[begin..i].iter().collect::<String>().parse::<isize>().unwrap_or(0)
                };
                if width < 0 { left = true; }
                let width = width.unsigned_abs();

                let precision = if chars.get(i) == Some(&'.') {
                    i += 1;
                    if chars.get(i) == Some(&'*') {
                        i += 1;
                        let raw = values.get(value_index).cloned().unwrap_or_default();
                        value_index += 1;
                        Some(raw.parse::<isize>().unwrap_or(0).max(0) as usize)
                    } else {
                        let begin = i;
                        while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                        Some(chars[begin..i].iter().collect::<String>().parse::<usize>().unwrap_or(0))
                    }
                } else {
                    None
                };

                let long_modifier = if chars.get(i) == Some(&'l') {
                    i += 1;
                    true
                } else {
                    false
                };
                let mut spec = chars.get(i).copied().unwrap_or(' ');
                if i < chars.len() { i += 1; }
                if long_modifier {
                    spec = match spec {
                        's' => 'S',
                        'c' => 'C',
                        other => other,
                    };
                }

                let value = values.get(value_index).cloned().unwrap_or_default();
                if spec != '%' { value_index += 1; }

                let mut numeric = false;
                let mut rendered = match spec {
                    's' | 'S' => precision
                        .map(|limit| value.chars().take(limit).collect())
                        .unwrap_or(value),
                    'q' | 'Q' => {
                        let raw = if spec == 'Q' {
                            precision
                                .map(|limit| value.chars().take(limit).collect::<String>())
                                .unwrap_or(value)
                        } else {
                            value
                        };
                        if alternate { shell_single_quote(&raw) } else { shell_quote(&raw) }
                    }
                    'b' => {
                        let (decoded, stop) = decode_backslash_escapes(&value, true);
                        if stop { stop_all = true; }
                        decoded
                    }
                    'c' | 'C' => value.chars().next().map(|ch| ch.to_string()).unwrap_or_default(),
                    'd' | 'i' => {
                        numeric = true;
                        let number = parse_printf_integer(&value).unwrap_or(0);
                        let magnitude = number.unsigned_abs().to_string();
                        let digits = precision
                            .map(|p| format!("{:0>width$}", magnitude, width=p))
                            .unwrap_or(magnitude);
                        if number < 0 { format!("-{digits}") }
                        else if plus { format!("+{digits}") }
                        else if space { format!(" {digits}") }
                        else { digits }
                    }
                    'u' => {
                        numeric = true;
                        let number = parse_printf_integer(&value).unwrap_or(0) as u64;
                        let digits = number.to_string();
                        precision
                            .map(|p| format!("{:0>width$}", digits, width=p))
                            .unwrap_or(digits)
                    }
                    'o' => {
                        numeric = true;
                        let number = parse_printf_integer(&value).unwrap_or(0) as u64;
                        let mut digits = format!("{number:o}");
                        if let Some(p) = precision { digits = format!("{:0>width$}", digits, width=p); }
                        if alternate && !digits.starts_with('0') { digits.insert(0, '0'); }
                        digits
                    }
                    'x' | 'X' => {
                        numeric = true;
                        let number = parse_printf_integer(&value).unwrap_or(0) as u64;
                        let mut digits = if spec == 'x' { format!("{number:x}") } else { format!("{number:X}") };
                        if let Some(p) = precision { digits = format!("{:0>width$}", digits, width=p); }
                        if alternate && number != 0 {
                            digits = format!("{}{}", if spec == 'x' { "0x" } else { "0X" }, digits);
                        }
                        digits
                    }
                    'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                        numeric = true;
                        let number = value.parse::<f64>().unwrap_or(0.0);
                        let p = precision.unwrap_or(6);
                        let mut text = match spec {
                            'f' | 'F' => format!("{number:.p$}"),
                            'e' => format!("{number:.p$e}"),
                            'E' => format!("{number:.p$E}"),
                            'g' => format_printf_general(number, p, false),
                            'G' => format_printf_general(number, p, true),
                            _ => unreachable!(),
                        };
                        if number >= 0.0 {
                            if plus { text.insert(0, '+'); }
                            else if space { text.insert(0, ' '); }
                        }
                        text
                    }
                    '%' => "%".to_owned(),
                    other => {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            format!("printf: especificador inválido: %{other}\n"),
                            1,
                        ));
                    }
                };

                if width > rendered.chars().count() {
                    let pad = width - rendered.chars().count();
                    if left {
                        rendered.push_str(&" ".repeat(pad));
                    } else if zero && numeric && precision.is_none() {
                        let sign_len = usize::from(rendered.chars().next().is_some_and(|ch| matches!(ch, '-' | '+' | ' ')));
                        if sign_len == 1 {
                            let sign = rendered.remove(0);
                            rendered = format!("{sign}{}{}", "0".repeat(pad), rendered);
                        } else if rendered.starts_with("0x") || rendered.starts_with("0X") {
                            let prefix = rendered[..2].to_owned();
                            rendered = format!("{prefix}{}{}", "0".repeat(pad), &rendered[2..]);
                        } else {
                            rendered = format!("{}{}", "0".repeat(pad), rendered);
                        }
                    } else {
                        rendered = format!("{}{}", " ".repeat(pad), rendered);
                    }
                }

                output.push_str(&rendered);
            }

            if format_stops || value_index == pass_start || value_index >= values.len() {
                break;
            }
        }

        if let Some(name) = assign_to {
            self.env.set(name, output);
            Ok(ExecutionResult::success())
        } else {
            Ok(ExecutionResult::from_parts(output, String::new(), 0))
        }
    }

    fn builtin_read(&mut self, args: &[String], stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut prompt = String::new();
        let mut silent = false;
        let mut raw = false;
        let mut max_chars: Option<usize> = None;
        let mut exact_chars = false;
        let mut array_name: Option<String> = None;
        let mut delimiter = '\n';
        let mut timeout: Option<std::time::Duration> = None;
        let mut fd = 0i32;
        let mut initial = String::new();
        let mut variables = Vec::new();
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-p" => {
                    index += 1;
                    prompt = args.get(index).cloned().unwrap_or_default();
                }
                "-s" => silent = true,
                "-r" => raw = true,
                "-e" | "-E" => {
                    // Native Shell Shock Tool line editing is always available for
                    // interactive reads; -E shares the same editing path on Bash 5.3.
                }
                "-n" | "-N" => {
                    exact_chars = args[index] == "-N";
                    index += 1;
                    max_chars = args.get(index).and_then(|v| v.parse::<usize>().ok());
                }
                "-a" => {
                    index += 1;
                    array_name = args.get(index).cloned();
                }
                "-d" => {
                    index += 1;
                    delimiter = args.get(index)
                        .and_then(|value| value.chars().next())
                        .unwrap_or('\0');
                }
                "-t" => {
                    index += 1;
                    let seconds = args.get(index)
                        .and_then(|value| value.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    timeout = Some(std::time::Duration::from_secs_f64(seconds.max(0.0)));
                }
                "-u" => {
                    index += 1;
                    fd = args.get(index).and_then(|value| value.parse::<i32>().ok()).unwrap_or(-1);
                }
                "-i" => {
                    index += 1;
                    initial = args.get(index).cloned().unwrap_or_default();
                }
                "--" => {
                    variables.extend(args[index + 1..].iter().cloned());
                    break;
                }
                value if value.starts_with('-') => {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("read: opción no válida: {value}\n"),
                        2,
                    ));
                }
                value => variables.push(value.to_owned()),
            }
            index += 1;
        }

        let source = if fd != 0 && stdin.is_none() {
            self.host.read_fd(fd, delimiter, max_chars, timeout)?
        } else if let Some(bytes) = stdin {
            let text = String::from_utf8_lossy(bytes);
            let value = if delimiter == '\0' {
                text.split('\0').next().unwrap_or("").to_owned()
            } else {
                text.split(delimiter).next().unwrap_or("").to_owned()
            };
            Some(value)
        } else {
            self.host.read_input(
                &prompt,
                silent,
                delimiter,
                max_chars,
                timeout,
                &initial,
            )?
        };

        let Some(mut value) = source else {
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        };

        if let Some(max) = max_chars {
            value = value.chars().take(max).collect();
            if exact_chars && value.chars().count() < max {
                return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
            }
        }
        if !raw {
            value = collapse_read_backslashes(&value);
        }

        let fields = split_ifs(&value, &self.env.get("IFS"));
        if let Some(name) = array_name {
            self.env.set_array(name, fields);
            return Ok(ExecutionResult::success());
        }

        if variables.is_empty() { variables.push("REPLY".to_owned()); }
        if variables.len() == 1 {
            self.env.set(variables[0].clone(), value);
        } else {
            for (pos, name) in variables.iter().enumerate() {
                let assigned = if pos + 1 == variables.len() {
                    fields.get(pos..).unwrap_or(&[]).join(&self.env.ifs_first().to_string())
                } else {
                    fields.get(pos).cloned().unwrap_or_default()
                };
                self.env.set(name.clone(), assigned);
            }
        }
        Ok(ExecutionResult::success())
    }

    fn builtin_declare(&mut self, args: &[String], local: bool) -> Result<ExecutionResult> {
        let mut indexed = false;
        let mut associative = false;
        let mut readonly = false;
        let mut export = false;
        let mut print = false;
        let mut integer = false;
        let mut nameref = false;
        let mut lowercase = false;
        let mut uppercase = false;
        let mut trace = false;
        let mut global = false;
        let mut inherit = false;
        let mut functions = false;
        let mut function_names_only = false;
        let mut remove_attrs = Vec::new();
        let mut names = Vec::new();

        for arg in args {
            if (arg.starts_with('-') || arg.starts_with('+')) && arg.len() > 1 {
                let remove = arg.starts_with('+');
                for flag in arg[1..].chars() {
                    if remove {
                        remove_attrs.push(flag);
                        continue;
                    }
                    match flag {
                        'a' => indexed = true,
                        'A' => associative = true,
                        'r' => readonly = true,
                        'x' => export = true,
                        'p' => print = true,
                        'i' => integer = true,
                        'n' => nameref = true,
                        'l' => lowercase = true,
                        'u' => uppercase = true,
                        't' => trace = true,
                        'g' => global = true,
                        'I' => inherit = true,
                        'f' => functions = true,
                        'F' => { functions = true; function_names_only = true; },
                        _ => {}
                    }
                }
            } else {
                names.push(arg.clone());
            }
        }

        if functions {
            let selected = if names.is_empty() {
                let mut values = self.env.functions.keys().cloned().collect::<Vec<_>>();
                values.sort();
                values
            } else {
                names.clone()
            };

            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut status = 0;
            let mut mutate = readonly || export || trace;
            mutate |= remove_attrs.iter().any(|flag| matches!(flag, 'r' | 'x' | 't'));

            for name in selected {
                let Some(body) = self.env.functions.get(&name) else {
                    status = 1;
                    stderr.push_str(&format!("declare: {name}: no es una función\n"));
                    continue;
                };

                if remove_attrs.contains(&'r') && self.env.readonly_functions.contains(&name) {
                    status = 1;
                    stderr.push_str(&format!("declare: {name}: no se puede quitar readonly\n"));
                    continue;
                }
                if remove_attrs.contains(&'x') { self.env.exported_functions.remove(&name); }
                if remove_attrs.contains(&'t') { self.env.trace_functions.remove(&name); }
                if readonly { self.env.readonly_functions.insert(name.clone()); }
                if export { self.env.exported_functions.insert(name.clone()); }
                if trace { self.env.trace_functions.insert(name.clone()); }

                if function_names_only || print {
                    let mut attrs = String::from("-f");
                    if self.env.readonly_functions.contains(&name) { attrs.push('r'); }
                    if self.env.trace_functions.contains(&name) { attrs.push('t'); }
                    if self.env.exported_functions.contains(&name) { attrs.push('x'); }
                    if function_names_only && self.env.option_enabled("extdebug") {
                        let source = self.function_sources.get(&name)
                            .cloned()
                            .unwrap_or_else(|| self.env.script_name.clone());
                        stdout.push_str(&format!("declare {attrs} {name} 0 {source}\n"));
                    } else if function_names_only {
                        stdout.push_str(&format!("declare {attrs} {name}\n"));
                    } else {
                        stdout.push_str(&format!(
                            "{name} () {{\n    {}\n}}\n",
                            render_ast(body)
                        ));
                    }
                } else if !mutate {
                    stdout.push_str(&format!(
                        "{name} () {{\n    {}\n}}\n",
                        render_ast(body)
                    ));
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, stderr, status));
        }

        if print || names.is_empty() {
            let mut stdout = String::new();
            let mut all: Vec<_> = self.env.vars.keys()
                .chain(self.env.namerefs.keys())
                .cloned()
                .collect();
            all.sort();
            all.dedup();
            for name in all {
                let mut flags = String::new();
                if self.env.is_nameref(&name) { flags.push('n'); }
                if self.env.readonly.contains(&name) { flags.push('r'); }
                if self.env.integer_vars.contains(&name) { flags.push('i'); }
                if self.env.uppercase_vars.contains(&name) { flags.push('u'); }
                if self.env.lowercase_vars.contains(&name) { flags.push('l'); }
                if self.env.exported.contains_key(&name) { flags.push('x'); }
                let value = if self.env.is_nameref(&name) {
                    self.env.namerefs.get(&name).cloned().unwrap_or_default()
                } else {
                    self.env.get(&name)
                };
                stdout.push_str(&format!(
                    "declare {} {}={}\n",
                    if flags.is_empty() { "--".to_owned() } else { format!("-{flags}") },
                    name,
                    shell_quote(&value)
                ));
            }
            for (name, values) in &self.env.arrays {
                stdout.push_str(&format!("declare -a {name}=("));
                let present = self.env.array_present.get(name);
                for (index, value) in values.iter().enumerate() {
                    if present.is_some_and(|indices| indices.contains(&index)) {
                        stdout.push_str(&format!("[{index}]={} ", shell_quote(value)));
                    }
                }
                stdout.push_str(")\n");
            }
            for (name, values) in &self.env.assoc_arrays {
                stdout.push_str(&format!("declare -A {name}=("));
                for (key, value) in values {
                    stdout.push_str(&format!("[{}]={} ", shell_quote(key), shell_quote(value)));
                }
                stdout.push_str(")\n");
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        for item in names {
            if local && item == "-" {
                if !self.env.localize_shell_options() {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        "local: -: solo puede usarse dentro de una función\n".to_owned(),
                        1,
                    ));
                }
                continue;
            }

            let (name, mut value) = item.split_once('=')
                .map(|(n,v)| (n.to_owned(), Some(v.to_owned())))
                .unwrap_or((item.clone(), None));
            let make_local = local && !global;

            for flag in &remove_attrs {
                match flag {
                    'n' => { self.env.unset_nameref(&name); }
                    'i' => self.env.set_integer(&name, false),
                    'u' => self.env.set_uppercase(&name, false),
                    'l' => self.env.set_lowercase(&name, false),
                    't' => self.env.set_trace(&name, false),
                    'x' => { self.env.exported.remove(&name); }
                    _ => {}
                }
            }

            if nameref {
                let target = value.take().unwrap_or_default();
                let expanded = self.expand_scalar(&target)?;
                if make_local {
                    self.env.set_local_nameref(name.clone(), expanded);
                } else {
                    self.env.set_nameref(name.clone(), expanded);
                }
            } else if associative {
                if make_local {
                    self.env.declare_local_assoc(name.clone());
                } else if !self.env.assoc_arrays.contains_key(&name) {
                    self.env.declare_assoc(name.clone());
                }
            } else if indexed {
                if make_local {
                    self.env.set_local_array(name.clone(), Vec::new());
                } else if !self.env.arrays.contains_key(&name) {
                    self.env.set_array(name.clone(), Vec::new());
                }
            }

            if let Some(raw_value) = value.take() {
                if raw_value.starts_with('(') && raw_value.ends_with(')') && (indexed || associative) {
                    let body = &raw_value[1..raw_value.len() - 1];
                    let items = split_shell_words_relaxed(body)?;
                    if associative {
                        if make_local { self.env.declare_local_assoc(name.clone()); }
                        else { self.env.declare_assoc(name.clone()); }
                        for item in items {
                            if let Some((key, value)) = parse_array_entry(&item) {
                                let expanded = self.expand_scalar(&value)?;
                                self.env.assoc_arrays.entry(name.clone()).or_default().insert(key, expanded);
                            }
                        }
                    } else {
                        let mut slots: Vec<Option<String>> = Vec::new();
                        let mut next_index = 0usize;
                        for item in items {
                            if let Some((key, value)) = parse_array_entry(&item) {
                                if let Ok(index) = key.parse::<usize>() {
                                    if slots.len() <= index { slots.resize(index + 1, None); }
                                    slots[index] = Some(self.expand_scalar(&value)?);
                                    next_index = index.saturating_add(1);
                                } else {
                                    if slots.len() <= next_index { slots.resize(next_index + 1, None); }
                                    slots[next_index] = Some(self.expand_scalar(&item)?);
                                    next_index += 1;
                                }
                            } else {
                                if slots.len() <= next_index { slots.resize(next_index + 1, None); }
                                slots[next_index] = Some(self.expand_scalar(&item)?);
                                next_index += 1;
                            }
                        }
                        if make_local { self.env.set_local_sparse_array(name.clone(), slots); }
                        else { self.env.set_sparse_array(name.clone(), slots); }
                    }
                } else if !nameref {
                    let expanded = if integer {
                        self.evaluate_arithmetic_command(&raw_value)?.to_string()
                    } else {
                        self.expand_scalar(&raw_value)?
                    };
                    if make_local {
                        if inherit {
                            self.env.set_local_inherited(name.clone(), expanded);
                        } else {
                            self.env.set_local(name.clone(), expanded);
                        }
                    } else {
                        self.env.set(name.clone(), expanded);
                    }
                }
            } else if make_local && !nameref && !indexed && !associative {
                if inherit || self.env.option_enabled("localvar_inherit") {
                    self.env.inherit_local_binding(&name);
                } else {
                    self.env.localize_unset(&name);
                }
            }

            if integer { self.env.set_integer(&name, true); }
            if lowercase { self.env.set_lowercase(&name, true); }
            if uppercase { self.env.set_uppercase(&name, true); }
            if trace { self.env.set_trace(&name, true); }
            if readonly { self.env.set_readonly(&name); }
            if export { self.env.mark_exported(&name); }
        }

        Ok(ExecutionResult::success())
    }

    fn builtin_set(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.is_empty() {
            let mut rows = self.env.vars.iter().collect::<Vec<_>>();
            rows.sort_by_key(|(name, _)| *name);
            let mut stdout: String = rows.into_iter()
                .map(|(name, value)| format!("{name}={}\n", shell_quote(value)))
                .collect();
            let mut functions = self.env.functions.keys().cloned().collect::<Vec<_>>();
            functions.sort();
            for name in functions {
                if let Some(body) = self.env.functions.get(&name) {
                    stdout.push_str(&format!("{name} () {{\n    {}\n}}\n", render_ast(body)));
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
        }

        let mut positional_start = None;
        let mut index = 0usize;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--" {
                positional_start = Some(index + 1);
                break;
            }

            if arg == "-o" || arg == "+o" {
                let enable = arg.starts_with('-');
                if let Some(option) = args.get(index + 1) {
                    if !bash_shell_options().contains(&option.as_str()) {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            format!("set: {option}: opción inválida\n"),
                            2,
                        ));
                    }
                    set_shell_option(&mut self.env, option, enable);
                    index += 2;
                    continue;
                }

                let mut stdout = String::new();
                for option in bash_shell_options() {
                    let enabled = self.env.shell_options.contains(*option);
                    if enable {
                        stdout.push_str(&format!("{option:<16} {}\n", if enabled { "on" } else { "off" }));
                    } else {
                        stdout.push_str(&format!("set {}o {option}\n", if enabled { "-" } else { "+" }));
                    }
                }
                return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
            }

            if arg.starts_with('-') || arg.starts_with('+') {
                let enable = arg.starts_with('-');
                if arg == "-" || arg == "+" {
                    index += 1;
                    continue;
                }
                for flag in arg[1..].chars() {
                    let option = match flag {
                        'a' => "allexport",
                        'b' => "notify",
                        'e' => "errexit",
                        'f' => "noglob",
                        'h' => "hashall",
                        'k' => "keyword",
                        'p' => "privileged",
                        'H' => "histexpand",
                        'm' => "monitor",
                        'n' => "noexec",
                        'P' => "physical",
                        't' => "onecmd",
                        'u' => "nounset",
                        'v' => "verbose",
                        'x' => "xtrace",
                        'B' => "braceexpand",
                        'C' => "noclobber",
                        'E' => "errtrace",
                        'T' => "functrace",
                        _ => continue,
                    };
                    set_shell_option(&mut self.env, option, enable);
                }
                index += 1;
                continue;
            }

            positional_start = Some(index);
            break;
        }

        if let Some(start) = positional_start {
            self.env.positional = args[start..].to_vec();
        }
        Ok(ExecutionResult::success())
    }

    fn builtin_shopt(&mut self, args: &[String]) -> ExecutionResult {
        let enable = args.iter().any(|arg| arg == "-s");
        let disable = args.iter().any(|arg| arg == "-u");
        let quiet = args.iter().any(|arg| arg == "-q");
        let reusable = args.iter().any(|arg| arg == "-p");
        let shell_options = args.iter().any(|arg| arg == "-o");
        let names: Vec<_> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .cloned()
            .collect();

        if enable && disable {
            return ExecutionResult::from_parts(
                String::new(),
                "shopt: no se pueden usar -s y -u a la vez\n".to_owned(),
                2,
            );
        }

        let known = if shell_options {
            bash_shell_options()
        } else {
            bash_shopt_options()
        };

        let is_enabled = |env: &ShellEnvironment, name: &str| {
            if shell_options {
                env.shell_options.contains(name)
            } else {
                env.shopt_options.contains(name)
            }
        };

        // Bash lists only set/unset options when -s/-u is supplied without names.
        if names.is_empty() && (enable || disable) {
            let mut stdout = String::new();
            if !quiet {
                for name in known {
                    let enabled_now = is_enabled(&self.env, name);
                    if (enable && enabled_now) || (disable && !enabled_now) {
                        if reusable {
                            stdout.push_str(&format!(
                                "shopt {} {}\n",
                                if enabled_now { "-s" } else { "-u" },
                                name
                            ));
                        } else {
                            stdout.push_str(&format!(
                                "{name:<24}{}\n",
                                if enabled_now { "on" } else { "off" }
                            ));
                        }
                    }
                }
            }
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        let selected: Vec<String> = if names.is_empty() {
            known.iter().map(|value| (*value).to_owned()).collect()
        } else {
            names
        };

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut status = 0;

        for name in selected {
            if !known.contains(&name.as_str()) {
                stderr.push_str(&format!("shopt: {name}: nombre de opción inválido\n"));
                status = 1;
                continue;
            }

            if enable {
                if shell_options {
                    set_shell_option(&mut self.env, &name, true);
                } else if let Some(version) = name.strip_prefix("compat") {
                    for option in bash_shopt_options().iter().filter(|option| option.starts_with("compat")) {
                        self.env.shopt_options.remove(*option);
                    }
                    self.env.shopt_options.insert(name.clone());
                    if version.len() == 2 {
                        self.env.set("BASH_COMPAT", format!("{}.{}", &version[..1], &version[1..]));
                    }
                } else {
                    self.env.shopt_options.insert(name.clone());
                }
            } else if disable {
                if shell_options { set_shell_option(&mut self.env, &name, false); }
                else { self.env.shopt_options.remove(&name); }
            } else if !is_enabled(&self.env, &name) {
                status = 1;
            }

            if !quiet && !enable && !disable {
                let enabled_now = is_enabled(&self.env, &name);
                if reusable {
                    stdout.push_str(&format!(
                        "shopt {} {}\n",
                        if enabled_now { "-s" } else { "-u" },
                        name
                    ));
                } else {
                    stdout.push_str(&format!(
                        "{name:<24}{}\n",
                        if enabled_now { "on" } else { "off" }
                    ));
                }
            }
        }
        ExecutionResult::from_parts(stdout, stderr, status)
    }

    fn builtin_trap(&mut self, args: &[String]) -> ExecutionResult {
        if args.first().map(String::as_str) == Some("-l") {
            let stdout = bash_signal_names()
                .iter()
                .enumerate()
                .map(|(index, name)| format!("{:2}) SIG{}\n", index + 1, name))
                .collect();
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        if args.first().map(String::as_str) == Some("-P") {
            let mut stdout = String::new();
            let mut status = 0;
            for raw in &args[1..] {
                let signal = normalize_signal(raw);
                if let Some(action) = self.env.traps.get(&signal) {
                    stdout.push_str(&format!("{}\n", shell_quote(action)));
                } else {
                    status = 1;
                }
            }
            return ExecutionResult::from_parts(stdout, String::new(), status);
        }

        if args.is_empty() || args.first().map(String::as_str) == Some("-p") {
            let requested: Vec<String> = if args.first().map(String::as_str) == Some("-p") {
                args[1..].iter().map(|value| normalize_signal(value)).collect()
            } else {
                Vec::new()
            };
            let mut traps = self.env.traps.iter().collect::<Vec<_>>();
            traps.sort_by_key(|(signal, _)| *signal);
            let stdout = traps.into_iter()
                .filter(|(signal, _)| requested.is_empty() || requested.iter().any(|item| item == *signal))
                .map(|(signal, action)| format!("trap -- {} {}\n", shell_quote(action), signal))
                .collect();
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        if args.len() == 1 {
            self.env.traps.remove(&normalize_signal(&args[0]));
            return ExecutionResult::success();
        }

        let action = args[0].clone();
        for signal in &args[1..] {
            let signal = normalize_signal(signal);
            if action == "-" {
                self.env.traps.remove(&signal);
            } else {
                self.env.traps.insert(signal, action.clone());
            }
        }
        ExecutionResult::success()
    }

    fn builtin_mapfile(&mut self, args: &[String], stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut trim = false;
        let mut skip = 0usize;
        let mut count: Option<usize> = None;
        let mut origin: Option<usize> = None;
        let mut delimiter = '\n';
        let mut fd = 0i32;
        let mut callback: Option<String> = None;
        let mut quantum = 5000usize;
        let mut name = "MAPFILE".to_owned();
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-t" => trim = true,
                "-s" => { index += 1; skip = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(0); }
                "-n" => { index += 1; count = args.get(index).and_then(|v| v.parse::<usize>().ok()); }
                "-O" => { index += 1; origin = args.get(index).and_then(|v| v.parse::<usize>().ok()); }
                "-d" => {
                    index += 1;
                    delimiter = args.get(index).and_then(|v| v.chars().next()).unwrap_or('\0');
                }
                "-u" => { index += 1; fd = args.get(index).and_then(|v| v.parse().ok()).unwrap_or(-1); }
                "-C" => { index += 1; callback = args.get(index).cloned(); }
                "-c" => {
                    index += 1;
                    quantum = args.get(index).and_then(|v| v.parse::<usize>().ok()).unwrap_or(5000).max(1);
                }
                "--" => {
                    if let Some(value) = args.get(index + 1) { name = value.clone(); }
                    break;
                }
                value if value.starts_with('-') => return Ok(ExecutionResult::from_parts(
                    String::new(), format!("mapfile: opción no válida: {value}\n"), 2,
                )),
                value => name = value.to_owned(),
            }
            index += 1;
        }

        let text = if fd != 0 && stdin.is_none() {
            let mut all = String::new();
            loop {
                let Some(value) = self.host.read_fd(fd, delimiter, None, None)? else { break };
                all.push_str(&value);
                all.push(delimiter);
            }
            all
        } else if let Some(bytes) = stdin {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            let mut all = String::new();
            loop {
                let Some(value) = self.host.read_input("", false, delimiter, None, None, "")? else { break };
                all.push_str(&value);
                all.push(delimiter);
            }
            all
        };

        let mut records = if delimiter == '\0' {
            text.split('\0').map(str::to_owned).collect::<Vec<_>>()
        } else {
            text.split_inclusive(delimiter).map(str::to_owned).collect::<Vec<_>>()
        };

        if records.last().is_some_and(|value| value.is_empty()) { records.pop(); }
        records = records.into_iter().skip(skip).collect();
        if let Some(limit) = count {
            if limit > 0 { records.truncate(limit); }
        }
        if trim {
            for record in &mut records {
                if record.ends_with(delimiter) { record.pop(); }
                if delimiter == '\n' && record.ends_with('\r') { record.pop(); }
            }
        }

        let start = origin.unwrap_or(0);
        let mut target: Vec<Option<String>> = if origin.is_some() {
            self.env.arrays.get(&name)
                .map(|values| {
                    let present = self.env.array_present.get(&name);
                    values.iter().enumerate()
                        .map(|(index, value)| {
                            present.is_some_and(|indices| indices.contains(&index))
                                .then(|| value.clone())
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if target.len() < start { target.resize(start, None); }

        for (offset, record) in records.into_iter().enumerate() {
            let array_index = start + offset;
            if let Some(callback) = &callback {
                if offset % quantum == 0 {
                    let source = format!("{} {} {}", callback, array_index, shell_quote(&record));
                    let callback_result = self.execute_text(&source)?;
                    if callback_result.status != 0 { return Ok(callback_result); }
                }
            }
            if target.len() <= array_index { target.resize(array_index + 1, None); }
            target[array_index] = Some(record);
        }

        self.env.set_sparse_array(name, target);
        Ok(ExecutionResult::success())
    }

    fn describe_command(&mut self, name: &str, verbose: bool, path_only: bool, force_path: bool) -> Result<Option<String>> {
        if !force_path && !path_only {
            if let Some(value) = self.env.aliases.get(name) {
                return Ok(Some(if verbose {
                    format!("{name} es un alias de {}", shell_quote(value))
                } else {
                    name.to_owned()
                }));
            }
            if self.env.functions.contains_key(name) {
                return Ok(Some(if verbose {
                    format!("{name} es una función")
                } else {
                    name.to_owned()
                }));
            }
            if bash_keywords().contains(&name) {
                return Ok(Some(if verbose {
                    format!("{name} es una palabra reservada del shell")
                } else {
                    name.to_owned()
                }));
            }
            if self.shell_builtin_name(name) {
                return Ok(Some(if verbose {
                    format!("{name} es un builtin de shell")
                } else {
                    name.to_owned()
                }));
            }
        }

        match self.host.execute_builtin("which", &[name.to_owned()], &self.env.cwd, None)? {
            Some(result) if result.status == 0 => {
                let path = result.stdout.lines().next().unwrap_or("").trim().to_owned();
                Ok((!path.is_empty()).then_some(path))
            }
            _ => Ok(None),
        }
    }

    fn builtin_command(&mut self, args: &[String], stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut verbose = false;
        let mut query = false;
        let mut default_path = false;
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-V" => verbose = true,
                "-v" => query = true,
                "-p" => default_path = true,
                "--" => { index += 1; break; }
                value if value.starts_with('-') && value.len() > 1 => {
                    for flag in value[1..].chars() {
                        match flag {
                            'V' => verbose = true,
                            'v' => query = true,
                            'p' => default_path = true,
                            _ => return Ok(ExecutionResult::from_parts(
                                String::new(),
                                format!("command: -{flag}: opción inválida\n"),
                                2,
                            )),
                        }
                    }
                }
                _ => break,
            }
            index += 1;
        }

        if query || verbose {
            let mut stdout = String::new();
            let mut status = 0;
            for name in &args[index..] {
                match self.describe_command(name, verbose, false, false)? {
                    Some(description) => {
                        stdout.push_str(&description);
                        stdout.push('\n');
                    }
                    None => status = 1,
                }
            }
            return Ok(ExecutionResult::from_parts(stdout, String::new(), status));
        }

        let Some(name) = args.get(index) else {
            return Ok(ExecutionResult::success());
        };

        if default_path {
            // Windows has no POSIX getconf PATH. Restrict lookup to System32 and
            // the Windows directory before falling back to the host resolver.
            if let Some(root) = std::env::var_os("SystemRoot") {
                let candidates = [
                    PathBuf::from(&root).join("System32").join(name),
                    PathBuf::from(&root).join(name),
                ];
                if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
                    let child_env = self.execution_environment();
                    return self.host.execute_external(
                        &path.to_string_lossy(),
                        &args[index + 1..],
                        &self.env.cwd,
                        &child_env,
                        stdin,
                    );
                }
            }
        }

        self.execute_command_direct(name, &args[index + 1..], stdin)
    }

    fn builtin_type(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut all = false;
        let mut short = false;
        let mut path_only = false;
        let mut force_path = false;
        let mut suppress_functions = false;
        let mut names = Vec::new();

        for arg in args {
            if arg == "--" { continue; }
            if arg.starts_with('-') && arg.len() > 1 {
                for flag in arg[1..].chars() {
                    match flag {
                        'a' => all = true,
                        't' => short = true,
                        'p' => path_only = true,
                        'P' => { path_only = true; force_path = true; },
                        'f' => suppress_functions = true,
                        _ => return Ok(ExecutionResult::from_parts(
                            String::new(),
                            format!("type: -{flag}: opción inválida\n"),
                            2,
                        )),
                    }
                }
            } else {
                names.push(arg.clone());
            }
        }

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut status = 0;

        for name in names {
            let mut found_any = false;

            if !force_path && !path_only {
                if let Some(value) = self.env.aliases.get(&name) {
                    found_any = true;
                    if short { stdout.push_str("alias\n"); }
                    else { stdout.push_str(&format!("{name} es un alias de {}\n", shell_quote(value))); }
                    if !all { continue; }
                }
                if !suppress_functions && self.env.functions.contains_key(&name) {
                    found_any = true;
                    if short { stdout.push_str("function\n"); }
                    else { stdout.push_str(&format!("{name} es una función\n")); }
                    if !all { continue; }
                }
                if bash_keywords().contains(&name.as_str()) {
                    found_any = true;
                    if short { stdout.push_str("keyword\n"); }
                    else { stdout.push_str(&format!("{name} es una palabra reservada del shell\n")); }
                    if !all { continue; }
                }
                if self.shell_builtin_name(&name) {
                    found_any = true;
                    if short { stdout.push_str("builtin\n"); }
                    else { stdout.push_str(&format!("{name} es un builtin de shell\n")); }
                    if !all { continue; }
                }
            }

            if force_path && all {
                if let Some(path) = self.command_hash.get(&name) {
                    found_any = true;
                    if short { stdout.push_str("file\n"); }
                    else { stdout.push_str(path); stdout.push('\n'); }
                }
            }

            match self.host.execute_builtin("which", &[name.clone()], &self.env.cwd, None)? {
                Some(result) if result.status == 0 => {
                    for path in result.stdout.lines().filter(|line| !line.trim().is_empty()) {
                        found_any = true;
                        if short { stdout.push_str("file\n"); }
                        else { stdout.push_str(path); stdout.push('\n'); }
                        if !all { break; }
                    }
                }
                _ => {}
            }

            if !found_any {
                status = 1;
                if !path_only {
                    stderr.push_str(&format!("type: {name}: no encontrado\n"));
                }
            }
        }

        Ok(ExecutionResult::from_parts(stdout, stderr, status))
    }

    fn builtin_getopts(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.len() < 2 {
            return Ok(ExecutionResult::from_parts(
                String::new(), "getopts: uso: getopts optstring name [args]\n".to_owned(), 2,
            ));
        }

        let optstring = &args[0];
        let varname = &args[1];
        let source = if args.len() > 2 { args[2..].to_vec() } else { self.env.positional.clone() };
        let mut optind = self.env.get("OPTIND").parse::<usize>().unwrap_or(1).max(1);
        let mut char_index = self.env.get("__SST_GETOPTS_POS").parse::<usize>().unwrap_or(1).max(1);
        let silent = optstring.starts_with(':');
        let definitions = if silent { &optstring[1..] } else { optstring.as_str() };

        let Some(item) = source.get(optind - 1) else {
            self.env.set(varname.clone(), "?");
            self.env.set("__SST_GETOPTS_POS", "1");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        };

        if item == "--" {
            optind += 1;
            self.env.set("OPTIND", optind.to_string());
            self.env.set("__SST_GETOPTS_POS", "1");
            self.env.set(varname.clone(), "?");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        }
        if !item.starts_with('-') || item == "-" {
            self.env.set(varname.clone(), "?");
            self.env.set("__SST_GETOPTS_POS", "1");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        }

        let mut option_chars: Vec<char> = item.chars().collect();
        if char_index >= option_chars.len() {
            optind += 1;
            char_index = 1;
        }
        let Some(item) = source.get(optind - 1) else {
            self.env.set(varname.clone(), "?");
            self.env.set("OPTIND", optind.to_string());
            self.env.set("__SST_GETOPTS_POS", "1");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        };
        option_chars = item.chars().collect();
        let option = option_chars.get(char_index).copied().unwrap_or('?');

        let definitions: Vec<char> = definitions.chars().collect();
        let Some(position) = definitions.iter().position(|ch| *ch == option) else {
            char_index += 1;
            if char_index >= option_chars.len() { optind += 1; char_index = 1; }
            self.env.set("OPTIND", optind.to_string());
            self.env.set("__SST_GETOPTS_POS", char_index.to_string());
            self.env.set(varname.clone(), "?");
            if silent {
                self.env.set("OPTARG", option.to_string());
                return Ok(ExecutionResult::success());
            }
            self.env.unset("OPTARG");
            let stderr = if self.env.get("OPTERR") != "0" {
                format!("{}: opción ilegal -- {}\n", self.env.script_name, option)
            } else { String::new() };
            return Ok(ExecutionResult::from_parts(String::new(), stderr, 0));
        };

        let requires_arg = definitions.get(position + 1) == Some(&':');
        self.env.set(varname.clone(), option.to_string());

        if requires_arg {
            if char_index + 1 < option_chars.len() {
                self.env.set("OPTARG", option_chars[char_index + 1..].iter().collect::<String>());
                optind += 1;
                char_index = 1;
            } else if let Some(argument) = source.get(optind) {
                self.env.set("OPTARG", argument.clone());
                optind += 2;
                char_index = 1;
            } else {
                optind += 1;
                self.env.set(varname.clone(), if silent { ":" } else { "?" });
                self.env.set("OPTARG", option.to_string());
                self.env.set("OPTIND", optind.to_string());
                self.env.set("__SST_GETOPTS_POS", "1");
                let stderr = if !silent && self.env.get("OPTERR") != "0" {
                    format!("{}: la opción requiere un argumento -- {}\n", self.env.script_name, option)
                } else { String::new() };
                return Ok(ExecutionResult::from_parts(String::new(), stderr, 0));
            }
        } else {
            self.env.unset("OPTARG");
            char_index += 1;
            if char_index >= option_chars.len() { optind += 1; char_index = 1; }
        }

        self.env.set("OPTIND", optind.to_string());
        self.env.set("__SST_GETOPTS_POS", char_index.to_string());
        Ok(ExecutionResult::success())
    }

    fn resolve_hashed_program(&mut self, name: &str) -> Result<String> {
        if let Some(path) = self.command_hash.get(name).cloned() {
            if !self.env.option_enabled("checkhash") || Path::new(&path).exists() {
                return Ok(path);
            }
            self.command_hash.remove(name);
        }

        if self.env.option_enabled("hashall") {
            if let Some(found) = self.host.execute_builtin(
                "which",
                &[name.to_owned()],
                &self.env.cwd,
                None,
            )? {
                if found.status == 0 {
                    if let Some(path) = found.stdout.lines().next().filter(|line| !line.is_empty()) {
                        self.command_hash.insert(name.to_owned(), path.to_owned());
                        return Ok(path.to_owned());
                    }
                }
            }
        }
        Ok(name.to_owned())
    }

    fn execute_command_direct(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        if name != "command" {
            if let Some(result) = self.shell_builtin(name, args, stdin)? {
                return Ok(result);
            }
        }
        if let Some(result) = self.host.execute_builtin(name, args, &self.env.cwd, stdin)? {
            return Ok(result);
        }
        let program = self.resolve_hashed_program(name)?;
        let child_env = self.execution_environment();
        match self.host.execute_external(&program, args, &self.env.cwd, &child_env, stdin) {
            Ok(result) => Ok(result),
            Err(error) => Ok(ExecutionResult::from_parts(String::new(), format!("{name}: {error}\n"), 127)),
        }
    }

    fn evaluate_conditional(&mut self, expression: &[String]) -> Result<bool> {
        fn split_top_level<'a>(
            items: &'a [String],
            operator: &str,
        ) -> Option<(&'a [String], &'a [String])> {
            let mut depth = 0i32;
            for (index, item) in items.iter().enumerate() {
                match item.as_str() {
                    "(" => depth += 1,
                    ")" => depth -= 1,
                    _ if depth == 0 && item == operator => {
                        return Some((&items[..index], &items[index + 1..]))
                    }
                    _ => {}
                }
            }
            None
        }

        if let Some((left, right)) = split_top_level(expression, "||") {
            return Ok(self.evaluate_conditional(left)? || self.evaluate_conditional(right)?);
        }
        if let Some((left, right)) = split_top_level(expression, "&&") {
            return Ok(self.evaluate_conditional(left)? && self.evaluate_conditional(right)?);
        }

        let mut items = expression;
        if items.first().map(String::as_str) == Some("(")
            && items.last().map(String::as_str) == Some(")")
        {
            items = &items[1..items.len() - 1];
        }

        if items.first().map(String::as_str) == Some("!") {
            return Ok(!self.evaluate_conditional(&items[1..])?);
        }

        match items {
            [] => Ok(false),
            [value] => Ok(!self.expand_scalar(value)?.is_empty()),
            [op, value] => {
                if op == "-v" {
                    let name = self.expand_scalar(value)?;
                    return Ok(self.env.is_set(&name));
                }

                let value = self.expand_scalar(value)?;
                let path = self.resolve_path(&value);
                Ok(match op.as_str() {
                    "-n" => !value.is_empty(),
                    "-z" => value.is_empty(),
                    "-e" | "-a" => path.exists(),
                    "-f" => path.is_file(),
                    "-d" => path.is_dir(),
                    "-s" => fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false),
                    "-r" => fs::File::open(&path).is_ok(),
                    "-w" => OpenOptions::new().write(true).open(&path).is_ok(),
                    "-x" => path.is_file(),
                    "-L" | "-h" => fs::symlink_metadata(&path)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false),
                    "-b" => self.host.file_type_test(&path, 'b')?.unwrap_or(false),
                    "-c" => self.host.file_type_test(&path, 'c')?.unwrap_or(false),
                    "-p" => self.host.file_type_test(&path, 'p')?.unwrap_or(false),
                    "-S" => self.host.file_type_test(&path, 'S')?.unwrap_or(false),
                    "-N" => fs::metadata(&path)
                        .ok()
                        .and_then(|meta| Some(meta.modified().ok()? > meta.accessed().ok()?))
                        .unwrap_or(false),
                    "-O" | "-G" => path.exists(),
                    "-g" | "-k" | "-u" => false,
                    "-R" => self.env.is_nameref(&value),
                    "-t" => value.parse::<i32>().ok().is_some_and(|fd| self.host.fd_is_terminal(fd)),
                    "-o" => self.env.option_enabled(&value),
                    _ => false,
                })
            }
            [left, op, right] => {
                let left = self.expand_scalar(left)?;
                let right = self.expand_scalar(right)?;
                let nocase = self.env.option_enabled("nocasematch");
                Ok(match op.as_str() {
                    "=" | "==" => {
                        let (l, r) = if nocase {
                            (left.to_lowercase(), right.to_lowercase())
                        } else {
                            (left.clone(), right.clone())
                        };
                        glob::Pattern::new(&r)
                            .map(|pattern| pattern.matches(&l))
                            .unwrap_or(l == r)
                    }
                    "!=" => {
                        let (l, r) = if nocase {
                            (left.to_lowercase(), right.to_lowercase())
                        } else {
                            (left.clone(), right.clone())
                        };
                        glob::Pattern::new(&r)
                            .map(|pattern| !pattern.matches(&l))
                            .unwrap_or(l != r)
                    }
                    "=~" => {
                        match regex::Regex::new(&right) {
                            Ok(pattern) => {
                                if let Some(captures) = pattern.captures(&left) {
                                    let values = (0..captures.len())
                                        .map(|index| captures.get(index)
                                            .map(|capture| capture.as_str().to_owned())
                                            .unwrap_or_default())
                                        .collect();
                                    self.env.set_array("BASH_REMATCH", values);
                                    true
                                } else {
                                    self.env.set_array("BASH_REMATCH", Vec::new());
                                    false
                                }
                            }
                            Err(_) => {
                                self.env.set_array("BASH_REMATCH", Vec::new());
                                false
                            }
                        }
                    },
                    "<" => if nocase { left.to_lowercase() < right.to_lowercase() } else { left < right },
                    ">" => if nocase { left.to_lowercase() > right.to_lowercase() } else { left > right },
                    "-eq" => eval_arithmetic(&left, &self.env)? == eval_arithmetic(&right, &self.env)?,
                    "-ne" => eval_arithmetic(&left, &self.env)? != eval_arithmetic(&right, &self.env)?,
                    "-lt" => eval_arithmetic(&left, &self.env)? < eval_arithmetic(&right, &self.env)?,
                    "-le" => eval_arithmetic(&left, &self.env)? <= eval_arithmetic(&right, &self.env)?,
                    "-gt" => eval_arithmetic(&left, &self.env)? > eval_arithmetic(&right, &self.env)?,
                    "-ge" => eval_arithmetic(&left, &self.env)? >= eval_arithmetic(&right, &self.env)?,
                    "-nt" => file_mtime(&self.resolve_path(&left)) > file_mtime(&self.resolve_path(&right)),
                    "-ot" => file_mtime(&self.resolve_path(&left)) < file_mtime(&self.resolve_path(&right)),
                    "-ef" => {
                        let a = fs::canonicalize(self.resolve_path(&left));
                        let b = fs::canonicalize(self.resolve_path(&right));
                        matches!((a,b),(Ok(a),Ok(b)) if a == b)
                    }
                    _ => false,
                })
            }
            _ => Ok(false),
        }
    }

    fn evaluate_arithmetic_command(&mut self, expression: &str) -> Result<i64> {
        let expression = expression.trim();
        if expression.is_empty() { return Ok(0); }

        let comma_parts = split_arithmetic_top_level(expression, ',');
        if comma_parts.len() > 1 {
            let mut value = 0;
            for part in comma_parts {
                value = self.evaluate_arithmetic_command(part)?;
            }
            return Ok(value);
        }

        for suffix in ["++", "--"] {
            if let Some(name) = expression.strip_suffix(suffix).map(str::trim) {
                if is_arithmetic_lvalue(name) {
                    let current = self.env.get(name).parse::<i64>().unwrap_or(0);
                    let next = if suffix == "++" { current.wrapping_add(1) } else { current.wrapping_sub(1) };
                    if !self.env.set(name.to_owned(), next.to_string()) {
                        bail!("{name}: variable de solo lectura");
                    }
                    return Ok(current);
                }
            }
        }

        for prefix in ["++", "--"] {
            if let Some(name) = expression.strip_prefix(prefix).map(str::trim) {
                if is_arithmetic_lvalue(name) {
                    let current = self.env.get(name).parse::<i64>().unwrap_or(0);
                    let next = if prefix == "++" { current.wrapping_add(1) } else { current.wrapping_sub(1) };
                    if !self.env.set(name.to_owned(), next.to_string()) {
                        bail!("{name}: variable de solo lectura");
                    }
                    return Ok(next);
                }
            }
        }

        if let Some((name, operator, rhs)) = find_arithmetic_assignment(expression) {
            let right = self.evaluate_arithmetic_command(rhs)?;
            let current = self.env.get(name).parse::<i64>().unwrap_or(0);
            let value = match operator {
                "=" => right,
                "+=" => current.wrapping_add(right),
                "-=" => current.wrapping_sub(right),
                "*=" => current.wrapping_mul(right),
                "/=" => {
                    if right == 0 { bail!("división por cero"); }
                    current / right
                }
                "%=" => {
                    if right == 0 { bail!("división por cero"); }
                    current % right
                }
                "<<=" => current.wrapping_shl(right.max(0) as u32),
                ">>=" => current.wrapping_shr(right.max(0) as u32),
                "&=" => current & right,
                "^=" => current ^ right,
                "|=" => current | right,
                "**=" => {
                    if right < 0 { 0 } else { current.wrapping_pow(right as u32) }
                }
                _ => right,
            };
            if !self.env.set(name.to_owned(), value.to_string()) {
                bail!("{name}: variable de solo lectura");
            }
            return Ok(value);
        }

        eval_arithmetic(expression, &self.env)
    }

    fn execute_xargs(
        &mut self,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let mut nul_delimited = false;
        let mut no_run_if_empty = false;
        let mut max_args: Option<usize> = None;
        let mut replacement: Option<String> = None;
        let mut command_start = 0usize;
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-0" | "--null" => {
                    nul_delimited = true;
                    index += 1;
                }
                "-r" | "--no-run-if-empty" => {
                    no_run_if_empty = true;
                    index += 1;
                }
                "-n" | "--max-args" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "xargs: -n requiere un número\n".to_owned(),
                            2,
                        ));
                    };
                    let count = value.parse::<usize>().unwrap_or(0);
                    if count == 0 {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "xargs: -n requiere un número mayor que cero\n".to_owned(),
                            2,
                        ));
                    }
                    max_args = Some(count);
                    index += 1;
                }
                "-I" | "--replace" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "xargs: -I requiere marcador\n".to_owned(),
                            2,
                        ));
                    };
                    replacement = Some(value.clone());
                    index += 1;
                }
                "--" => {
                    command_start = index + 1;
                    break;
                }
                value if value.starts_with('-') => {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("xargs: opción no soportada: {value}\n"),
                        2,
                    ));
                }
                _ => {
                    command_start = index;
                    break;
                }
            }
        }

        if index >= args.len() {
            command_start = args.len();
        }

        let input = stdin.unwrap_or_default();
        let items: Vec<String> = if nul_delimited {
            input
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect()
        } else if replacement.is_some() {
            String::from_utf8_lossy(input)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        } else {
            String::from_utf8_lossy(input)
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        };

        if items.is_empty() && no_run_if_empty {
            return Ok(ExecutionResult::success());
        }

        let base_command = if command_start < args.len() {
            args[command_start..].to_vec()
        } else {
            vec!["echo".to_owned()]
        };

        if base_command.is_empty() {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                "xargs: falta comando\n".to_owned(),
                2,
            ));
        }

        let mut combined = ExecutionResult::success();

        if let Some(marker) = replacement {
            if items.is_empty() {
                return Ok(combined);
            }

            for item in items {
                let words = base_command
                    .iter()
                    .map(|word| word.replace(&marker, &item))
                    .collect::<Vec<_>>();
                let command = words
                    .iter()
                    .map(|word| shell_quote(word))
                    .collect::<Vec<_>>()
                    .join(" ");
                let result = self.execute_text(&command)?;
                combined.append(result);
                if combined.exit_requested || combined.status != 0 {
                    break;
                }
            }

            return Ok(combined);
        }

        let chunk_size = max_args.unwrap_or_else(|| items.len().max(1));
        if items.is_empty() {
            let command = base_command
                .iter()
                .map(|word| shell_quote(word))
                .collect::<Vec<_>>()
                .join(" ");
            return self.execute_text(&command);
        }

        for chunk in items.chunks(chunk_size) {
            let mut words = base_command.clone();
            words.extend(chunk.iter().cloned());
            let command = words
                .iter()
                .map(|word| shell_quote(word))
                .collect::<Vec<_>>()
                .join(" ");
            let result = self.execute_text(&command)?;
            combined.append(result);
            if combined.exit_requested || combined.status != 0 {
                break;
            }
        }

        Ok(combined)
    }

    fn apply_output_redirects(
        &mut self,
        command: &SimpleCommand,
        result: &mut ExecutionResult,
    ) -> Result<()> {
        let mut routes = self.persistent_output_routes.clone();
        routes.entry(1).or_insert(OutputSink::Stdout);
        routes.entry(2).or_insert(OutputSink::Stderr);
        let mut prepared_files: HashSet<PathBuf> = HashSet::new();

        for redirect in &command.redirects {
            match redirect.kind {
                RedirectKind::Write | RedirectKind::Clobber | RedirectKind::Append => {
                    let target = self.expand_scalar(&redirect.target)?;
                    let path = self.resolve_path(&target);
                    if redirect.kind == RedirectKind::Write
                        && self.env.option_enabled("noclobber")
                        && path.exists()
                    {
                        bail!("{target}: no se puede sobrescribir: noclobber activo");
                    }
                    let append = redirect.kind == RedirectKind::Append;
                    let mut options = OpenOptions::new();
                    options.create(true).write(true);
                    if append { options.append(true); } else { options.truncate(true); }
                    let _ = options.open(&path)?;
                    prepared_files.insert(path.clone());
                    routes.insert(redirect.fd, OutputSink::File(path, append));
                }
                RedirectKind::BothWrite | RedirectKind::BothAppend => {
                    let target = self.expand_scalar(&redirect.target)?;
                    let path = self.resolve_path(&target);
                    if redirect.kind == RedirectKind::BothWrite
                        && self.env.option_enabled("noclobber")
                        && path.exists()
                    {
                        bail!("{target}: no se puede sobrescribir: noclobber activo");
                    }
                    let append = redirect.kind == RedirectKind::BothAppend;
                    let mut options = OpenOptions::new();
                    options.create(true).write(true);
                    if append { options.append(true); } else { options.truncate(true); }
                    let _ = options.open(&path)?;
                    prepared_files.insert(path.clone());
                    let sink = OutputSink::File(path, append);
                    routes.insert(1, sink.clone());
                    routes.insert(2, sink);
                }
                RedirectKind::DupOutput => {
                    let target = self.expand_scalar(&redirect.target)?;
                    let sink = if target == "-" {
                        OutputSink::Closed
                    } else if let Ok(fd) = target.parse::<i32>() {
                        routes.get(&fd).cloned().unwrap_or(OutputSink::HostFd(fd))
                    } else {
                        let path = self.resolve_path(&target);
                        let _ = OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(&path)?;
                        prepared_files.insert(path.clone());
                        OutputSink::File(path, false)
                    };
                    routes.insert(redirect.fd, sink);
                }
                RedirectKind::Read
                | RedirectKind::DupInput
                | RedirectKind::ReadWrite
                | RedirectKind::HereString => {}
            }
        }

        let original_stdout = std::mem::take(&mut result.stdout);
        let original_stderr = std::mem::take(&mut result.stderr);
        let mut routed_stdout = String::new();
        let mut routed_stderr = String::new();
        let mut written_files: HashSet<PathBuf> = HashSet::new();

        for (fd, data) in [(1, original_stdout), (2, original_stderr)] {
            if data.is_empty() { continue; }
            let sink = routes.get(&fd).cloned().unwrap_or_else(|| {
                if fd == 2 { OutputSink::Stderr } else { OutputSink::Stdout }
            });

            match sink {
                OutputSink::Stdout => routed_stdout.push_str(&data),
                OutputSink::Stderr => routed_stderr.push_str(&data),
                OutputSink::Closed => {}
                OutputSink::HostFd(target_fd) => {
                    if !self.host.write_fd(target_fd, data.as_bytes())? {
                        return Err(anyhow!("{fd}>&{target_fd}: descriptor no disponible"));
                    }
                }
                OutputSink::File(path, _append) => {
                    // The redirection itself already created/truncated the file in
                    // left-to-right order. Data is appended now so duplicated
                    // descriptors that share the same sink do not erase each other.
                    let mut file = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)?;
                    file.write_all(data.as_bytes())?;
                    written_files.insert(path);
                }
            }
        }

        // Keep the sets semantically used: prepared files model redirection side
        // effects even when the command produces no bytes.
        let _ = (prepared_files, written_files);
        if self.persist_next_redirections {
            self.persistent_output_routes = routes;
            self.persist_next_redirections = false;
        }
        result.stdout = routed_stdout;
        result.stderr = routed_stderr;
        Ok(())
    }

    fn expand_words(&mut self, words: &[String]) -> Result<Vec<String>> {
        let mut result = Vec::new();

        for raw in words {
            let brace_values = if self.env.option_enabled("braceexpand") {
                brace_expand(raw)
            } else {
                vec![raw.clone()]
            };
            for braced in brace_values {
                if braced == "\"$@\"" {
                    result.extend(self.env.positional.clone());
                    continue;
                }
                if braced == "\"$*\"" {
                    result.push(self.env.positional.join(&self.env.ifs_first().to_string()));
                    continue;
                }
                if let Some(name) = quoted_array_expansion(&braced, "@") {
                    result.extend(self.env.array_values(&name));
                    continue;
                }
                if let Some(name) = quoted_array_expansion(&braced, "*") {
                    result.push(self.env.array_values(&name).join(&self.env.ifs_first().to_string()));
                    continue;
                }

                let quoted = is_shell_quoted(&braced);
                let process_substitution = (braced.starts_with("<(") || braced.starts_with(">("))
                    && braced.ends_with(')');
                let expanded = self.expand_scalar(&braced)?;

                if quoted || process_substitution {
                    result.push(expanded);
                    continue;
                }

                let fields = split_ifs(&expanded, &self.env.get("IFS"));
                if fields.is_empty() && !expanded.is_empty() {
                    continue;
                }
                for field in fields {
                    if self.env.option_enabled("noglob") {
                        result.push(field);
                        continue;
                    }
                    let paths = self.glob(&field)?;
                    if paths.is_empty() {
                        if self.env.option_enabled("failglob") && contains_glob_meta(&field) {
                            bail!("no hay coincidencias: {field}");
                        }
                        if !self.env.option_enabled("nullglob") || !contains_glob_meta(&field) {
                            result.push(field);
                        }
                    } else {
                        result.extend(paths);
                    }
                }
            }
        }

        Ok(result)
    }

    fn expand_scalar(&mut self, raw: &str) -> Result<String> {
        let raw = self.tilde_expand(raw);
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        let mut single = false;
        let mut double = false;

        while i < chars.len() {
            if !single && chars[i] == '$' && chars.get(i + 1) == Some(&'\'') {
                let mut end = i + 2;
                let mut escaped = false;
                while end < chars.len() {
                    if escaped {
                        escaped = false;
                    } else if chars[end] == '\\' {
                        escaped = true;
                    } else if chars[end] == '\'' {
                        break;
                    }
                    end += 1;
                }
                if end >= chars.len() { bail!("comilla ANSI-C sin cerrar"); }
                let body: String = chars[i + 2..end].iter().collect();
                out.push_str(&decode_backslash_escapes(&body, false).0);
                i = end + 1;
                continue;
            }

            // Locale-translated double-quoted strings. Shell Shock Tool does not
            // ship gettext catalogs, so an untranslated msgid follows Bash's
            // fallback semantics while retaining normal double-quote expansion.
            if !single && chars[i] == '$' && chars.get(i + 1) == Some(&'"') {
                let mut end = i + 2;
                let mut escaped = false;
                while end < chars.len() {
                    if escaped {
                        escaped = false;
                    } else if chars[end] == '\\' {
                        escaped = true;
                    } else if chars[end] == '"' {
                        break;
                    }
                    end += 1;
                }
                if end >= chars.len() {
                    bail!("cadena localizada sin cerrar");
                }
                let body: String = chars[i + 2..end].iter().collect();
                out.push_str(&self.expand_scalar(&format!("\"{body}\""))?);
                i = end + 1;
                continue;
            }

            if !single
                && matches!(chars[i], '<' | '>')
                && chars.get(i + 1) == Some(&'(')
            {
                let direction = chars[i];
                let end = matching(&chars, i + 1, '(', ')')
                    .ok_or_else(|| anyhow!("sustitución de proceso sin cerrar"))?;
                let source: String = chars[i + 2..end].iter().collect();
                let path = self.create_process_substitution(direction, &source)?;
                out.push_str(&path.to_string_lossy());
                i = end + 1;
                continue;
            }

            match chars[i] {
                '\'' if !double => { single = !single; i += 1; }
                '"' if !single => { double = !double; i += 1; }
                '\\' if !single && i + 1 < chars.len() => {
                    let next = chars[i + 1];
                    if !double || matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                        if next != '\n' { out.push(next); }
                        i += 2;
                    } else {
                        out.push('\\');
                        i += 1;
                    }
                }
                '`' if !single => {
                    let mut end = i + 1;
                    while end < chars.len() && chars[end] != '`' { end += 1; }
                    if end >= chars.len() { bail!("sustitución con backticks sin cerrar"); }
                    let source: String = chars[i + 1..end].iter().collect();
                    let saved = self.env.clone();
                    if !self.env.option_enabled("inherit_errexit")
                        && !self.env.option_enabled("posix")
                    {
                        self.env.shell_options.remove("errexit");
                    }
                    let result = self.execute_text(&source);
                    self.env = saved;
                    let result = result?;
                    out.push_str(result.stdout.trim_end_matches(['\r','\n']));
                    i = end + 1;
                }
                '$' if !single => {
                    if chars.get(i + 1) == Some(&'(') && chars.get(i + 2) == Some(&'(') {
                        let end = arithmetic_end(&chars, i + 3)
                            .ok_or_else(|| anyhow!("expansión aritmética sin cerrar"))?;
                        let expression: String = chars[i + 3..end].iter().collect();
                        out.push_str(&self.evaluate_arithmetic_command(&expression)?.to_string());
                        i = end + 2;
                    } else if chars.get(i + 1) == Some(&'(') {
                        let end = matching(&chars, i + 1, '(', ')')
                            .ok_or_else(|| anyhow!("sustitución de comando sin cerrar"))?;
                        let source: String = chars[i + 2..end].iter().collect();
                        let saved = self.env.clone();
                        if !self.env.option_enabled("inherit_errexit")
                            && !self.env.option_enabled("posix")
                        {
                            self.env.shell_options.remove("errexit");
                        }
                        let result = self.execute_text(&source);
                        self.env = saved;
                        let result = result?;
                        out.push_str(result.stdout.trim_end_matches(['\r', '\n']));
                        i = end + 1;
                    } else if chars.get(i + 1) == Some(&'{') {
                        let end = matching(&chars, i + 1, '{', '}')
                            .ok_or_else(|| anyhow!("expansión de parámetro sin cerrar"))?;
                        let expression: String = chars[i + 2..end].iter().collect();
                        out.push_str(&self.expand_parameter(&expression)?);
                        i = end + 1;
                    } else {
                        let (name, used) = parameter_name(&chars[i + 1..]);
                        if used == 0 {
                            out.push('$');
                            i += 1;
                        } else {
                            if self.env.option_enabled("nounset")
                                && !special_parameter(&name)
                                && !self.env.is_set(&name)
                            {
                                bail!("{name}: variable no definida");
                            }
                            out.push_str(&self.special_value(&name));
                            i += used + 1;
                        }
                    }
                }
                ch => { out.push(ch); i += 1; }
            }
        }

        if single || double { bail!("comillas sin cerrar"); }
        Ok(out)
    }


    fn create_process_substitution(&mut self, direction: char, source: &str) -> Result<PathBuf> {
        let child_env = self.execution_environment();
        if let Some(path) = self.host.create_process_substitution_pipe(
            direction,
            source,
            &self.env.cwd,
            &child_env,
        )? {
            return Ok(path);
        }

        // Portable fallback for hosts without stream-backed process substitution.
        self.process_substitution_counter = self.process_substitution_counter.wrapping_add(1);
        let path = std::env::temp_dir().join(format!(
            "sst-process-substitution-{}-{}.tmp",
            std::process::id(),
            self.process_substitution_counter
        ));

        if direction == '<' {
            let saved = self.env.clone();
            let result = self.execute_text(source);
            self.env = saved;
            let result = result?;
            fs::write(&path, result.stdout.as_bytes())?;
            self.process_substitutions.push(ProcessSubstitution {
                path: path.clone(),
                command: String::new(),
                consume_as_stdin: false,
            });
        } else {
            fs::write(&path, b"")?;
            self.process_substitutions.push(ProcessSubstitution {
                path: path.clone(),
                command: source.to_owned(),
                consume_as_stdin: true,
            });
        }

        Ok(path)
    }

    fn finalize_process_substitutions(&mut self, result: &mut ExecutionResult) -> Result<()> {
        let pending = std::mem::take(&mut self.process_substitutions);
        for substitution in pending {
            if substitution.consume_as_stdin {
                let input = fs::read(&substitution.path).unwrap_or_default();
                let node = parse(&substitution.command)?;
                let consumer = self.execute(&node, Some(&input))?;
                result.stdout.push_str(&consumer.stdout);
                result.stderr.push_str(&consumer.stderr);
                if result.status == 0 && consumer.status != 0 {
                    result.status = consumer.status;
                }
            }
            let _ = fs::remove_file(&substitution.path);
        }
        Ok(())
    }

    fn special_value(&self, name: &str) -> String {
        match name {
            "BASHPID" | "$" => std::process::id().to_string(),
            _ => self.env.get(name),
        }
    }

    fn expand_parameter(&mut self, expression: &str) -> Result<String> {
        if let Some(source) = expression.strip_prefix('|') {
            let source = source.trim();
            let source = source.strip_suffix(';').unwrap_or(source).trim();
            let _ = self.execute_text(source)?;
            return Ok(self.env.get("REPLY"));
        }

        if expression.starts_with(char::is_whitespace) {
            let source = expression.trim();
            let source = source.strip_suffix(';').unwrap_or(source).trim();
            let result = self.execute_text(source)?;
            return Ok(result.stdout.trim_end_matches(['\r', '\n']).to_owned());
        }

        if let Some(rest) = expression.strip_prefix('!') {
            if let Some(base) = rest.strip_suffix("[@]").or_else(|| rest.strip_suffix("[*]")) {
                return Ok(self.env.array_keys(base).join(" "));
            }
            if let Some(prefix) = rest.strip_suffix('*').or_else(|| rest.strip_suffix('@')) {
                let mut names: Vec<String> = self.env.vars.keys()
                    .chain(self.env.arrays.keys())
                    .chain(self.env.assoc_arrays.keys())
                    .filter(|name| name.starts_with(prefix))
                    .cloned()
                    .collect();
                names.sort();
                names.dedup();
                return Ok(names.join(" "));
            }
            let indirect = self.env.get(rest);
            if self.env.option_enabled("nounset") && !self.env.is_set(&indirect) {
                bail!("{indirect}: variable no definida");
            }
            return Ok(self.env.get(&indirect));
        }

        if let Some(name) = expression.strip_prefix('#') {
            if let Some(base) = name.strip_suffix("[@]").or_else(|| name.strip_suffix("[*]")) {
                return Ok(self.env.array_values(base).len().to_string());
            }
            return Ok(self.env.get(name).chars().count().to_string());
        }

        for suffix in ["^^", "^", ",,", ","] {
            if let Some(name) = expression.strip_suffix(suffix) {
                let value = self.env.get(name);
                return Ok(match suffix {
                    "^^" => value.to_uppercase(),
                    "^" => capitalize_first(&value),
                    ",," => value.to_lowercase(),
                    "," => lowercase_first(&value),
                    _ => value,
                });
            }
        }

        for suffix in ["@Q", "@E", "@P", "@A", "@a", "@K", "@U", "@u", "@L"] {
            if let Some(name) = expression.strip_suffix(suffix) {
                let value = self.env.get(name);
                return Ok(match suffix {
                    "@Q" => shell_quote(&value),
                    "@E" => decode_backslash_escapes(&value, false).0,
                    "@P" => self.expand_prompt_text(&value)?,
                    "@U" => value.to_uppercase(),
                    "@u" => capitalize_first(&value),
                    "@L" => value.to_lowercase(),
                    "@a" => {
                        let base = name.split('[').next().unwrap_or(name);
                        let mut attrs = String::new();
                        if self.env.assoc_arrays.contains_key(base) { attrs.push('A'); }
                        else if self.env.arrays.contains_key(base) { attrs.push('a'); }
                        if self.env.is_nameref(base) { attrs.push('n'); }
                        if self.env.readonly.contains(base) { attrs.push('r'); }
                        if self.env.integer_vars.contains(base) { attrs.push('i'); }
                        if self.env.uppercase_vars.contains(base) { attrs.push('u'); }
                        if self.env.lowercase_vars.contains(base) { attrs.push('l'); }
                        if self.env.trace_vars.contains(base) { attrs.push('t'); }
                        if self.env.exported.contains_key(base) { attrs.push('x'); }
                        attrs
                    }
                    "@A" => {
                        let base = name.split('[').next().unwrap_or(name);
                        if let Some(values) = self.env.assoc_arrays.get(base) {
                            let mut keys = values.keys().cloned().collect::<Vec<_>>();
                            keys.sort();
                            let body = keys.into_iter()
                                .map(|key| format!(
                                    "[{}]={}",
                                    shell_quote(&key),
                                    shell_quote(values.get(&key).map(String::as_str).unwrap_or(""))
                                ))
                                .collect::<Vec<_>>()
                                .join(" ");
                            format!("declare -A {base}=({body})")
                        } else if let Some(values) = self.env.arrays.get(base) {
                            let present = self.env.array_present.get(base);
                            let body = values.iter().enumerate()
                                .filter(|(index, _)| present.is_none_or(|indices| indices.contains(index)))
                                .map(|(index, item)| format!("[{index}]={}", shell_quote(item)))
                                .collect::<Vec<_>>()
                                .join(" ");
                            format!("declare -a {base}=({body})")
                        } else if self.env.is_nameref(base) {
                            format!(
                                "declare -n {base}={}",
                                shell_quote(self.env.namerefs.get(base).map(String::as_str).unwrap_or(""))
                            )
                        } else {
                            let mut attrs = String::new();
                            if self.env.readonly.contains(base) { attrs.push('r'); }
                            if self.env.integer_vars.contains(base) { attrs.push('i'); }
                            if self.env.uppercase_vars.contains(base) { attrs.push('u'); }
                            if self.env.lowercase_vars.contains(base) { attrs.push('l'); }
                            if self.env.trace_vars.contains(base) { attrs.push('t'); }
                            if self.env.exported.contains_key(base) { attrs.push('x'); }
                            format!(
                                "declare {} {base}={}",
                                if attrs.is_empty() { "--".to_owned() } else { format!("-{attrs}") },
                                shell_quote(&value)
                            )
                        }
                    }
                    "@K" => {
                        let base = name.split('[').next().unwrap_or(name);
                        if let Some(values) = self.env.assoc_arrays.get(base) {
                            let mut keys = values.keys().cloned().collect::<Vec<_>>();
                            keys.sort();
                            keys.into_iter()
                                .flat_map(|key| {
                                    let item = values.get(&key).cloned().unwrap_or_default();
                                    [shell_quote(&key), shell_quote(&item)]
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else if let Some(values) = self.env.arrays.get(base) {
                            let present = self.env.array_present.get(base);
                            values.iter().enumerate()
                                .filter(|(index, _)| present.is_none_or(|indices| indices.contains(index)))
                                .flat_map(|(index, item)| [index.to_string(), shell_quote(item)])
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else {
                            shell_quote(&value)
                        }
                    }
                    _ => value,
                });
            }
        }

        for operator in [":-", ":+", ":=", ":?", "-", "+", "=", "?"] {
            if let Some((name, word)) = split_parameter_operator(expression, operator) {
                let is_set = self.env.is_set(name);
                let value = self.env.get(name);
                let null_counts = operator.starts_with(':');
                let missing = !is_set || (null_counts && value.is_empty());
                let expanded_word = if word.is_empty() { String::new() } else { self.expand_scalar(word)? };
                return match operator {
                    ":-" | "-" => Ok(if missing { expanded_word } else { value }),
                    ":+" | "+" => Ok(if missing { String::new() } else { expanded_word }),
                    ":=" | "=" => {
                        if missing {
                            if !self.env.set(name.to_owned(), expanded_word.clone()) {
                                bail!("{name}: variable de solo lectura");
                            }
                            Ok(expanded_word)
                        } else {
                            Ok(value)
                        }
                    }
                    ":?" | "?" => {
                        if missing {
                            let message = if expanded_word.is_empty() { format!("{name}: parámetro nulo o no definido") } else { expanded_word };
                            bail!("{message}")
                        } else {
                            Ok(value)
                        }
                    }
                    _ => Ok(value),
                };
            }
        }

        if let Some((name, rest)) = split_substring_expression(expression) {
            let value = self.env.get(name);
            let mut parts = rest.splitn(2, ':');
            let offset = eval_arithmetic(parts.next().unwrap_or("0").trim(), &self.env)?;
            let length = parts.next().map(|part| eval_arithmetic(part.trim(), &self.env)).transpose()?;
            return Ok(substring_chars(&value, offset, length));
        }

        let base_len = parameter_reference_len(expression);
        let (name, remainder) = expression.split_at(base_len);

        if let Some(rest) = remainder.strip_prefix("//") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob(&self.env.get(name), pattern, replacement, true, self.env.option_enabled("patsub_replacement")));
        }
        if let Some(rest) = remainder.strip_prefix("/#") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob_anchored(&self.env.get(name), pattern, replacement, true, self.env.option_enabled("patsub_replacement")));
        }
        if let Some(rest) = remainder.strip_prefix("/%") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob_anchored(&self.env.get(name), pattern, replacement, false, self.env.option_enabled("patsub_replacement")));
        }
        if let Some(rest) = remainder.strip_prefix('/') {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob(&self.env.get(name), pattern, replacement, false, self.env.option_enabled("patsub_replacement")));
        }

        for operator in ["##", "#", "%%", "%"] {
            if let Some(pattern) = remainder.strip_prefix(operator) {
                return Ok(remove_glob_pattern(&self.env.get(name), pattern, operator));
            }
        }

        if self.env.option_enabled("nounset")
            && !special_parameter(name)
            && !self.env.is_set(name)
        {
            bail!("{name}: variable no definida");
        }

        Ok(self.special_value(name))
    }

    fn tilde_expand(&self, raw: &str) -> String {
        if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
            let home = self.env.get("HOME");
            let home = if home.is_empty() { self.env.get("USERPROFILE") } else { home };
            if !home.is_empty() {
                return format!("{home}{}", &raw[1..]);
            }
        }
        if raw == "~+" {
            return self.env.cwd.to_string_lossy().into_owned();
        }
        if raw == "~-" {
            return self.env.oldpwd.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
        }

        if let Some(number) = raw.strip_prefix("~+").and_then(|value| value.parse::<usize>().ok()) {
            let stack = self.directory_stack();
            return stack.get(number)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw.to_owned());
        }
        if let Some(number) = raw.strip_prefix("~-").and_then(|value| value.parse::<usize>().ok()) {
            let stack = self.directory_stack();
            return stack.len().checked_sub(number + 1)
                .and_then(|index| stack.get(index))
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw.to_owned());
        }

        if let Some(user) = raw.strip_prefix('~').filter(|value| !value.is_empty() && !value.contains(['/', '\\'])) {
            if user.eq_ignore_ascii_case(&self.env.get("USERNAME")) {
                let home = self.env.get("USERPROFILE");
                if !home.is_empty() { return home; }
            }
        }

        raw.to_owned()
    }

    fn directory_stack(&self) -> Vec<PathBuf> {
        let mut stack = vec![self.env.cwd.clone()];
        stack.extend(self.env.dir_stack.iter().rev().cloned());
        stack
    }

    fn glob(&self, value: &str) -> Result<Vec<String>> {
        if !contains_glob_meta(value) && !contains_extglob(value) {
            return Ok(Vec::new());
        }

        let absolute_pattern = if Path::new(value).is_absolute() {
            value.to_owned()
        } else {
            self.env.cwd.join(value).to_string_lossy().into_owned()
        };

        let use_extglob = self.env.option_enabled("extglob") && contains_extglob(value);
        let broad_pattern = if use_extglob {
            extglob_broad_pattern(&absolute_pattern)
        } else {
            absolute_pattern.clone()
        };

        let globignore = self.env.get("GLOBIGNORE");
        let implicit_dotglob = !globignore.is_empty();
        let options = glob::MatchOptions {
            case_sensitive: !self.env.option_enabled("nocaseglob"),
            require_literal_separator: !self.env.option_enabled("globstar"),
            require_literal_leading_dot: !(self.env.option_enabled("dotglob") || implicit_dotglob),
        };

        let filter = if use_extglob {
            Some(bash_glob_regex(
                &normalize_glob_path(&absolute_pattern),
                self.env.option_enabled("nocaseglob"),
            )?)
        } else {
            None
        };

        let mut result = Vec::new();
        for entry in glob::glob_with(&broad_pattern, options)? {
            let Ok(path) = entry else { continue };
            let normalized = normalize_glob_path(&path.to_string_lossy());
            if let Some(regex) = &filter {
                if !regex.is_match(&normalized) {
                    continue;
                }
                if negative_extglob_rejects(
                    &normalize_glob_path(&absolute_pattern),
                    &normalized,
                    self.env.option_enabled("nocaseglob"),
                ) {
                    continue;
                }
            }

            if !self.env.option_enabled("dotglob") && !implicit_dotglob {
                let hidden = path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'));
                let explicit_hidden = Path::new(value).file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'));
                if hidden && !explicit_hidden { continue; }
            }

            let candidate = if Path::new(value).is_absolute() {
                path.to_string_lossy().into_owned()
            } else if let Ok(relative) = path.strip_prefix(&self.env.cwd) {
                relative.to_string_lossy().into_owned()
            } else {
                continue;
            };

            if !globignore.is_empty() {
                let normalized = normalize_glob_path(&candidate);
                let basename = Path::new(&candidate)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&candidate);
                if basename == "." || basename == ".." {
                    continue;
                }
                let ignored = globignore.split(':')
                    .filter(|pattern| !pattern.is_empty())
                    .any(|pattern| {
                        let options = glob::MatchOptions {
                            case_sensitive: !self.env.option_enabled("nocaseglob"),
                            require_literal_separator: false,
                            require_literal_leading_dot: false,
                        };
                        glob::Pattern::new(pattern)
                            .map(|compiled| {
                                compiled.matches_with(&normalized, options)
                                    || compiled.matches_with(basename, options)
                            })
                            .unwrap_or(false)
                    });
                if ignored { continue; }
            }

            result.push(candidate);
        }

        sort_glob_results(&mut result, &self.env.get("GLOBSORT"), &self.env.cwd);
        Ok(result)
    }

    fn resolve_path(&self, raw: &str) -> PathBuf {
        if raw == "~" {
            let home = self.env.get("USERPROFILE");
            if !home.is_empty() { return PathBuf::from(home); }
        }
        if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            let home = self.env.get("USERPROFILE");
            if !home.is_empty() { return PathBuf::from(home).join(rest); }
        }
        #[cfg(windows)]
        if raw.len() >= 3 && raw.starts_with('/') && raw.as_bytes()[2] == b'/' {
            let drive = raw.chars().nth(1).unwrap_or('c').to_ascii_uppercase();
            return PathBuf::from(format!("{drive}:\\")).join(raw[3..].replace('/', "\\"));
        }
        let path = PathBuf::from(raw);
        if path.is_absolute() { path } else { self.env.cwd.join(path) }
    }
}



#[derive(Debug, Clone)]
struct HeredocSpec {
    start: usize,
    end: usize,
    delimiter: String,
    strip_tabs: bool,
    quoted: bool,
}

fn heredoc_specs(line: &str) -> Vec<HeredocSpec> {
    let bytes = line.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0usize;
    let mut single = false;
    let mut double = false;

    while i + 1 < bytes.len() {
        match bytes[i] {
            b'\'' if !double => { single = !single; i += 1; continue; }
            b'"' if !single => { double = !double; i += 1; continue; }
            b'\\' => { i = (i + 2).min(bytes.len()); continue; }
            _ => {}
        }
        if single || double || bytes[i] != b'<' || bytes[i + 1] != b'<' || bytes.get(i + 2) == Some(&b'<') {
            i += 1;
            continue;
        }

        let start = i;
        i += 2;
        let strip_tabs = bytes.get(i) == Some(&b'-');
        if strip_tabs { i += 1; }
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') { i += 1; }
        let delim_start = i;
        let mut quoted = false;
        let delimiter = if matches!(bytes.get(i), Some(b'\'') | Some(b'"')) {
            quoted = true;
            let quote = bytes[i];
            i += 1;
            let content_start = i;
            while i < bytes.len() && bytes[i] != quote { i += 1; }
            let value = String::from_utf8_lossy(&bytes[content_start..i]).into_owned();
            if i < bytes.len() { i += 1; }
            value
        } else {
            while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b';' | b'|' | b'&') { i += 1; }
            String::from_utf8_lossy(&bytes[delim_start..i]).into_owned()
        };
        if !delimiter.is_empty() {
            specs.push(HeredocSpec { start, end: i, delimiter, strip_tabs, quoted });
        }
    }
    specs
}

fn strip_outer_quotes(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

fn parse_printf_integer(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('\'').or_else(|| value.strip_prefix('"')) {
        return rest.chars().next().map(|ch| ch as i64);
    }
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    if value.len() > 1 && value.starts_with('0') && value.chars().all(|ch| matches!(ch, '0'..='7')) {
        return i64::from_str_radix(&value[1..], 8).ok();
    }
    value.parse::<i64>().ok()
}

fn format_printf_general(value: f64, precision: usize, upper: bool) -> String {
    let precision = precision.max(1);
    let abs = value.abs();
    let exponent = if abs == 0.0 { 0 } else { abs.log10().floor() as i32 };
    let mut rendered = if exponent < -4 || exponent >= precision as i32 {
        let digits = precision.saturating_sub(1);
        if upper { format!("{value:.digits$E}") } else { format!("{value:.digits$e}") }
    } else {
        let decimals = (precision as i32 - exponent - 1).max(0) as usize;
        format!("{value:.decimals$}")
    };
    if rendered.contains('.') {
        while rendered.ends_with('0') { rendered.pop(); }
        if rendered.ends_with('.') { rendered.pop(); }
    }
    rendered
}

fn decode_backslash_escapes(input: &str, echo_mode: bool) -> (String, bool) {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut stop = false;
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        match chars[i] {
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'c' if echo_mode => { stop = true; break; }
            'e' | 'E' => out.push('\x1b'),
            'f' => out.push('\x0c'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\x0b'),
            '\\' => out.push('\\'),
            '0'..='7' => {
                let mut octal = String::new();
                if chars[i] != '0' || echo_mode { octal.push(chars[i]); }
                let mut count = 1usize;
                while i + 1 < chars.len() && count < 3 && matches!(chars[i + 1], '0'..='7') {
                    i += 1;
                    octal.push(chars[i]);
                    count += 1;
                }
                if octal.is_empty() { octal.push('0'); }
                if let Ok(value) = u8::from_str_radix(&octal, 8) { out.push(value as char); }
            }
            'x' => {
                let mut hex = String::new();
                while i + 1 < chars.len() && hex.len() < 2 && chars[i + 1].is_ascii_hexdigit() {
                    i += 1;
                    hex.push(chars[i]);
                }
                if let Ok(value) = u8::from_str_radix(&hex, 16) { out.push(value as char); }
            }
            'u' | 'U' => {
                let max = if chars[i] == 'u' { 4 } else { 8 };
                let mut hex = String::new();
                while i + 1 < chars.len() && hex.len() < max && chars[i + 1].is_ascii_hexdigit() {
                    i += 1;
                    hex.push(chars[i]);
                }
                if let Ok(value) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(value) { out.push(ch); }
                }
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
        i += 1;
    }
    (out, stop)
}

fn collapse_read_backslashes(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() { out.push(next); }
        } else {
            out.push(ch);
        }
    }
    out
}

fn split_ifs(input: &str, ifs: &str) -> Vec<String> {
    if ifs.is_empty() { return vec![input.to_owned()]; }
    let separators: Vec<char> = ifs.chars().collect();
    let whitespace_only = separators.iter().all(|ch| ch.is_whitespace());
    if whitespace_only {
        return input.split_whitespace().map(str::to_owned).collect();
    }

    let mut fields = Vec::new();
    let mut current = String::new();
    let mut saw_non_ws_sep = false;
    for ch in input.chars() {
        if separators.contains(&ch) {
            if !current.is_empty() || !ch.is_whitespace() || saw_non_ws_sep {
                fields.push(std::mem::take(&mut current));
            }
            saw_non_ws_sep = !ch.is_whitespace();
        } else {
            current.push(ch);
            saw_non_ws_sep = false;
        }
    }
    if !current.is_empty() || saw_non_ws_sep { fields.push(current); }
    fields
}

fn render_timeformat(format: &str, real_seconds: f64, user_seconds: f64, system_seconds: f64) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'%') {
            out.push('%');
            i += 2;
            continue;
        }

        i += 1;
        let mut precision = 3usize;
        if chars.get(i).is_some_and(|ch| ch.is_ascii_digit()) {
            precision = chars[i].to_digit(10).unwrap_or(3).min(6) as usize;
            i += 1;
        }
        let long = if chars.get(i) == Some(&'l') {
            i += 1;
            true
        } else {
            false
        };
        let spec = chars.get(i).copied().unwrap_or('%');
        if i < chars.len() { i += 1; }

        let seconds = match spec {
            'R' => Some(real_seconds),
            'U' => Some(user_seconds),
            'S' => Some(system_seconds),
            _ => None,
        };

        if let Some(value) = seconds {
            if long {
                let minutes = (value / 60.0).floor() as u64;
                let remainder = value - minutes as f64 * 60.0;
                out.push_str(&format!("{minutes}m{remainder:.precision$}s"));
            } else {
                out.push_str(&format!("{value:.precision$}"));
            }
        } else if spec == 'P' {
            let cpu = user_seconds + system_seconds;
            let percentage = if real_seconds > 0.0 { cpu * 100.0 / real_seconds } else { 0.0 };
            out.push_str(&format!("{percentage:.precision$}"));
        } else {
            out.push('%');
            if long { out.push('l'); }
            out.push(spec);
        }
    }
    out.push('\n');
    out
}

fn format_shell_cpu_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as u64;
    let remainder = seconds - minutes as f64 * 60.0;
    format!("{minutes}m{remainder:.3}s")
}

fn set_shell_option(env: &mut ShellEnvironment, name: &str, enabled: bool) {
    if enabled {
        env.shell_options.insert(name.to_owned());
        if name == "posix" {
            env.shopt_options.insert("inherit_errexit".to_owned());
            env.shopt_options.insert("expand_aliases".to_owned());
            env.vars.insert("POSIXLY_CORRECT".to_owned(), "y".to_owned());
            env.exported.insert("POSIXLY_CORRECT".to_owned(), "y".to_owned());
        }
    } else {
        env.shell_options.remove(name);
        if name == "posix" {
            env.vars.remove("POSIXLY_CORRECT");
            env.exported.remove("POSIXLY_CORRECT");
        }
    }
}

fn bash_signal_names() -> &'static [&'static str] {
    &[
        "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE",
        "KILL", "USR1", "SEGV", "USR2", "PIPE", "ALRM", "TERM",
        "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU",
        "URG", "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO",
        "PWR", "SYS",
    ]
}

fn signal_number(signal: &str) -> usize {
    match signal {
        "EXIT" => 0,
        "DEBUG" | "RETURN" | "ERR" => 0,
        other => bash_signal_names().iter()
            .position(|name| *name == other)
            .map(|index| index + 1)
            .unwrap_or(0),
    }
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmpty for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

fn parse_symbolic_umask(value: &str) -> Option<u16> {
    let mut allowed = 0o777u16;
    for clause in value.split(',') {
        let (who, op, perms) = {
            let index = clause.find(['=', '+', '-'])?;
            (&clause[..index], clause.as_bytes()[index] as char, &clause[index + 1..])
        };
        let who = if who.is_empty() { "a" } else { who };
        let classes = [
            ('u', 6u16),
            ('g', 3u16),
            ('o', 0u16),
        ];
        for (class, shift) in classes {
            if !who.contains(class) && !who.contains('a') { continue; }
            let mut bits = 0u16;
            for perm in perms.chars() {
                bits |= match perm {
                    'r' => 0o4,
                    'w' => 0o2,
                    'x' => 0o1,
                    _ => return None,
                };
            }
            let class_mask = 0o7u16 << shift;
            match op {
                '=' => {
                    allowed &= !class_mask;
                    allowed |= bits << shift;
                }
                '+' => allowed |= bits << shift,
                '-' => allowed &= !(bits << shift),
                _ => return None,
            }
        }
    }
    Some(0o777 & !allowed)
}

fn normalize_signal(value: &str) -> String {
    let upper = value.trim_start_matches("SIG").to_ascii_uppercase();
    if let Ok(number) = upper.parse::<usize>() {
        if number == 0 { return "EXIT".to_owned(); }
        return bash_signal_names()
            .get(number.saturating_sub(1))
            .copied()
            .unwrap_or("UNKNOWN")
            .to_owned();
    }
    upper
}

fn is_shell_quoted(value: &str) -> bool {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped { escaped = false; continue; }
        if ch == '\\' && !single { escaped = true; continue; }
        if ch == '\'' && !double { single = !single; continue; }
        if ch == '"' && !single { double = !double; continue; }
        if single || double { return true; }
    }
    value.contains('\'') || value.contains('"')
}

fn quoted_array_expansion(raw: &str, suffix: &str) -> Option<String> {
    let prefix = "\"$".to_owned() + "{";
    let wrapped_prefix = format!("\"{prefix}");
    if !raw.starts_with(&wrapped_prefix) || !raw.ends_with("}\"") { return None; }
    let inner = &raw[wrapped_prefix.len()..raw.len() - 2];
    let needle = format!("[{suffix}]");
    inner.strip_suffix(&needle).map(str::to_owned)
}

fn contains_glob_meta(value: &str) -> bool {
    value.contains('*') || value.contains('?') || value.contains('[')
}

fn special_parameter(name: &str) -> bool {
    matches!(
        name,
        "?" | "#" | "@" | "*" | "!" | "$" | "-" | "RANDOM" | "BASH_VERSION" | "BASHPID" | "PPID"
    ) || name.chars().all(|ch| ch.is_ascii_digit())
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn lowercase_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn split_parameter_operator<'a>(expression: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let index = expression.find(operator)?;
    if index == 0 { return None; }
    let name = &expression[..index];
    if parameter_reference_len(name) != name.len() { return None; }
    Some((name, &expression[index + operator.len()..]))
}

fn split_substring_expression(expression: &str) -> Option<(&str, &str)> {
    let base_len = parameter_reference_len(expression);
    if base_len == 0 || expression.as_bytes().get(base_len) != Some(&b':') { return None; }
    let rest = &expression[base_len + 1..];
    if rest.starts_with(['-', '+', '=', '?']) { return None; }
    Some((&expression[..base_len], rest))
}

fn substring_chars(value: &str, offset: i64, length: Option<i64>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len() as i64;
    let start = if offset < 0 { (len + offset).max(0) } else { offset.min(len) };
    let end = match length {
        Some(length) if length < 0 => (len + length).max(start),
        Some(length) => (start + length).min(len),
        None => len,
    };
    chars[start as usize..end as usize].iter().collect()
}

fn parameter_reference_len(expression: &str) -> usize {
    let chars: Vec<char> = expression.chars().collect();
    if chars.is_empty() { return 0; }
    if matches!(chars[0], '?' | '#' | '@' | '*' | '!' | '$' | '-') || chars[0].is_ascii_digit() {
        return chars[0].len_utf8();
    }

    let mut chars_seen = 0usize;
    let mut bytes_seen = 0usize;
    for ch in expression.chars() {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            chars_seen += 1;
            bytes_seen += ch.len_utf8();
        } else {
            break;
        }
    }
    if chars_seen == 0 { return 0; }

    let tail = &expression[bytes_seen..];
    if tail.starts_with('[') {
        if let Some(close) = tail.find(']') {
            bytes_seen += close + 1;
        }
    }
    bytes_seen
}

fn char_boundaries(value: &str) -> Vec<usize> {
    let mut points: Vec<usize> = value.char_indices().map(|(i,_)| i).collect();
    points.push(value.len());
    points
}

fn remove_glob_pattern(value: &str, pattern: &str, operator: &str) -> String {
    let Ok(pattern) = glob::Pattern::new(pattern) else { return value.to_owned(); };
    let points = char_boundaries(value);
    match operator {
        "#" | "##" => {
            let ordered: Vec<usize> = if operator == "#" {
                points.clone()
            } else {
                points.iter().copied().rev().collect()
            };
            for point in ordered {
                if pattern.matches(&value[..point]) { return value[point..].to_owned(); }
            }
        }
        "%" | "%%" => {
            let ordered: Vec<usize> = if operator == "%" {
                points.iter().copied().rev().collect()
            } else {
                points.clone()
            };
            for point in ordered {
                if pattern.matches(&value[point..]) { return value[..point].to_owned(); }
            }
        }
        _ => {}
    }
    value.to_owned()
}

fn render_pattern_replacement(replacement: &str, matched: &str, expand_match: bool) -> String {
    if !expand_match {
        return replacement.to_owned();
    }

    let mut out = String::new();
    let mut escaped = false;
    for ch in replacement.chars() {
        if escaped {
            if ch == '&' || ch == '\\' {
                out.push(ch);
            } else {
                out.push('\\');
                out.push(ch);
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '&' {
            out.push_str(matched);
        } else {
            out.push(ch);
        }
    }
    if escaped { out.push('\\'); }
    out
}

fn replace_glob(
    value: &str,
    pattern: &str,
    replacement: &str,
    all: bool,
    expand_match: bool,
) -> String {
    let Ok(pattern) = glob::Pattern::new(pattern) else { return value.to_owned(); };
    let points = char_boundaries(value);
    let mut out = String::new();
    let mut cursor = 0usize;

    while cursor < value.len() {
        let mut found = None;
        'outer: for &start in points.iter().filter(|&&p| p >= cursor) {
            for &end in points.iter().filter(|&&p| p > start) {
                if pattern.matches(&value[start..end]) {
                    found = Some((start,end));
                    break 'outer;
                }
            }
        }
        let Some((start,end)) = found else {
            out.push_str(&value[cursor..]);
            break;
        };
        out.push_str(&value[cursor..start]);
        let matched = &value[start..end];
        out.push_str(&render_pattern_replacement(replacement, matched, expand_match));
        cursor = end;
        if !all {
            out.push_str(&value[cursor..]);
            break;
        }
    }
    if value.is_empty() { String::new() } else { out }
}

fn replace_glob_anchored(
    value: &str,
    pattern: &str,
    replacement: &str,
    prefix: bool,
    expand_match: bool,
) -> String {
    let Ok(pattern) = glob::Pattern::new(pattern) else { return value.to_owned(); };
    let points = char_boundaries(value);
    if prefix {
        for &end in points.iter().rev() {
            if pattern.matches(&value[..end]) {
                let rendered = render_pattern_replacement(replacement, &value[..end], expand_match);
                return format!("{rendered}{}", &value[end..]);
            }
        }
    } else {
        for &start in &points {
            if pattern.matches(&value[start..]) {
                let rendered = render_pattern_replacement(replacement, &value[start..], expand_match);
                return format!("{}{rendered}", &value[..start]);
            }
        }
    }
    value.to_owned()
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() { return "''".to_owned(); }
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\' | ':' | '@' | '%')
    }) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn is_variable_name(name: &str) -> bool {
    !name.is_empty() && name.chars().enumerate().all(|(index, ch)| {
        ch == '_' || (ch.is_ascii_alphanumeric() && (index > 0 || !ch.is_ascii_digit()))
    })
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else { return false };
    let base = name.split('[').next().unwrap_or(name);
    is_variable_name(base)
}

fn brace_expand(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut single = false;
    let mut double = false;
    let mut start = None;
    let mut depth = 0usize;

    for (index, ch) in chars.iter().copied().enumerate() {
        match ch {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '{' if !single && !double => {
                if depth == 0 { start = Some(index); }
                depth += 1;
            }
            '}' if !single && !double && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let start = start.unwrap();
                    let inner: String = chars[start + 1..index].iter().collect();
                    let prefix: String = chars[..start].iter().collect();
                    let suffix: String = chars[index + 1..].iter().collect();

                    if let Some(range) = brace_range(&inner) {
                        return range.into_iter()
                            .flat_map(|part| brace_expand(&format!("{prefix}{part}{suffix}")))
                            .collect();
                    }

                    if inner.contains(',') {
                        return split_brace_alternatives(&inner).into_iter()
                            .flat_map(|part| brace_expand(&format!("{prefix}{part}{suffix}")))
                            .collect();
                    }
                }
            }
            _ => {}
        }
    }
    vec![input.to_owned()]
}

fn split_brace_alternatives(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in inner.chars() {
        match ch {
            '{' => { depth += 1; current.push(ch); }
            '}' => { depth = depth.saturating_sub(1); current.push(ch); }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

fn brace_range(inner: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = inner.split("..").collect();
    if !(2..=3).contains(&parts.len()) { return None; }
    let step = parts.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    if step == 0 { return None; }

    if let (Ok(start), Ok(end)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
        let width = parts[0].trim_start_matches('-').len().max(parts[1].trim_start_matches('-').len());
        let padded = parts[0].trim_start_matches('-').starts_with('0')
            || parts[1].trim_start_matches('-').starts_with('0');
        let actual_step = if start <= end { step.abs() } else { -step.abs() };
        let mut value = start;
        let mut out = Vec::new();
        while (actual_step > 0 && value <= end) || (actual_step < 0 && value >= end) {
            if padded {
                let sign = if value < 0 { "-" } else { "" };
                out.push(format!("{sign}{:0width$}", value.abs(), width=width));
            } else {
                out.push(value.to_string());
            }
            value += actual_step;
        }
        return Some(out);
    }

    let mut left = parts[0].chars();
    let mut right = parts[1].chars();
    if let (Some(start), None, Some(end), None) = (left.next(), left.next(), right.next(), right.next()) {
        let start = start as i64;
        let end = end as i64;
        let actual_step = if start <= end { step.abs() } else { -step.abs() };
        let mut value = start;
        let mut out = Vec::new();
        while (actual_step > 0 && value <= end) || (actual_step < 0 && value >= end) {
            if let Some(ch) = char::from_u32(value as u32) { out.push(ch.to_string()); }
            value += actual_step;
        }
        return Some(out);
    }
    None
}

fn matching(chars: &[char], start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut single = false;
    let mut double = false;
    for (index, ch) in chars.iter().copied().enumerate().skip(start) {
        match ch {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            ch if !single && !double && ch == open => depth += 1,
            ch if !single && !double && ch == close => {
                depth -= 1;
                if depth == 0 { return Some(index); }
            }
            _ => {}
        }
    }
    None
}

fn arithmetic_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while index + 1 < chars.len() {
        match chars[index] {
            '(' => depth += 1,
            ')' if depth == 0 && chars[index + 1] == ')' => return Some(index),
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    None
}

fn parameter_name(chars: &[char]) -> (String, usize) {
    if let Some(ch) = chars.first() {
        if matches!(ch, '?' | '#' | '@' | '*' | '!' | '$' | '-') || ch.is_ascii_digit() {
            return (ch.to_string(), 1);
        }
    }
    let mut len = 0usize;
    for ch in chars {
        if *ch == '_' || ch.is_ascii_alphanumeric() { len += 1; } else { break; }
    }
    (chars[..len].iter().collect(), len)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ArithmeticToken {
    Number(i64),
    Ident(String),
    Op(String),
    LParen,
    RParen,
    Question,
    Colon,
    Comma,
    End,
}

fn tokenize_arithmetic(expression: &str) -> Result<Vec<ArithmeticToken>> {
    let chars: Vec<char> = expression.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i].is_whitespace() { i += 1; continue; }

        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '#' || chars[i] == 'x' || chars[i] == 'X') {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            tokens.push(ArithmeticToken::Number(parse_arithmetic_number(&raw)?));
            continue;
        }

        if chars[i] == '_' || chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i] == '_' || chars[i].is_ascii_alphanumeric()) { i += 1; }
            if chars.get(i) == Some(&'[') {
                let mut depth = 1usize;
                i += 1;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '[' => depth += 1,
                        ']' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 { bail!("subíndice aritmético sin cerrar"); }
            }
            tokens.push(ArithmeticToken::Ident(chars[start..i].iter().collect()));
            continue;
        }

        let remaining: String = chars[i..].iter().collect();
        let mut found = None;
        for op in ["**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||"] {
            if remaining.starts_with(op) {
                found = Some(op);
                break;
            }
        }
        if let Some(op) = found {
            tokens.push(ArithmeticToken::Op(op.to_owned()));
            i += op.len();
            continue;
        }

        match chars[i] {
            '(' => tokens.push(ArithmeticToken::LParen),
            ')' => tokens.push(ArithmeticToken::RParen),
            '?' => tokens.push(ArithmeticToken::Question),
            ':' => tokens.push(ArithmeticToken::Colon),
            ',' => tokens.push(ArithmeticToken::Comma),
            '+' | '-' | '*' | '/' | '%' | '<' | '>' | '&' | '^' | '|' | '!' | '~' => {
                tokens.push(ArithmeticToken::Op(chars[i].to_string()));
            }
            ch => bail!("operador aritmético no soportado: {ch}"),
        }
        i += 1;
    }
    tokens.push(ArithmeticToken::End);
    Ok(tokens)
}

fn parse_arithmetic_number(raw: &str) -> Result<i64> {
    if let Some((base, digits)) = raw.split_once('#') {
        let base = base.parse::<u32>()?;
        if !(2..=64).contains(&base) { bail!("base aritmética inválida: {base}"); }
        let mut value = 0i64;
        for ch in digits.chars() {
            let digit = match ch {
                '0'..='9' => ch as u32 - '0' as u32,
                'a'..='z' => 10 + ch as u32 - 'a' as u32,
                'A'..='Z' => 36 + ch as u32 - 'A' as u32,
                '@' => 62,
                '_' => 63,
                _ => bail!("dígito inválido para base {base}: {ch}"),
            };
            if digit >= base { bail!("dígito inválido para base {base}: {ch}"); }
            value = value.saturating_mul(base as i64).saturating_add(digit as i64);
        }
        return Ok(value);
    }
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        return Ok(i64::from_str_radix(hex, 16)?);
    }
    if raw.len() > 1 && raw.starts_with('0') && raw.chars().all(|ch| matches!(ch, '0'..='7')) {
        return Ok(i64::from_str_radix(&raw[1..], 8)?);
    }
    Ok(raw.parse::<i64>()?)
}

struct ArithmeticParser<'a> {
    tokens: Vec<ArithmeticToken>,
    pos: usize,
    env: &'a ShellEnvironment,
}

impl<'a> ArithmeticParser<'a> {
    fn new(expression: &str, env: &'a ShellEnvironment) -> Result<Self> {
        Ok(Self { tokens: tokenize_arithmetic(expression)?, pos: 0, env })
    }

    fn peek(&self) -> &ArithmeticToken { self.tokens.get(self.pos).unwrap_or(&ArithmeticToken::End) }
    fn take(&mut self) -> ArithmeticToken {
        let token = self.peek().clone();
        self.pos += 1;
        token
    }
    fn op(&mut self, expected: &str) -> bool {
        if matches!(self.peek(), ArithmeticToken::Op(op) if op == expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse(mut self) -> Result<i64> { self.comma() }

    fn comma(&mut self) -> Result<i64> {
        let mut value = self.ternary()?;
        while matches!(self.peek(), ArithmeticToken::Comma) {
            self.pos += 1;
            value = self.ternary()?;
        }
        Ok(value)
    }

    fn ternary(&mut self) -> Result<i64> {
        let condition = self.logical_or()?;
        if matches!(self.peek(), ArithmeticToken::Question) {
            self.pos += 1;
            let yes = self.ternary()?;
            if !matches!(self.take(), ArithmeticToken::Colon) { bail!("operador ternario sin ':'"); }
            let no = self.ternary()?;
            Ok(if condition != 0 { yes } else { no })
        } else {
            Ok(condition)
        }
    }

    fn logical_or(&mut self) -> Result<i64> {
        let mut value = self.logical_and()?;
        while self.op("||") {
            let rhs = self.logical_and()?;
            value = ((value != 0) || (rhs != 0)) as i64;
        }
        Ok(value)
    }

    fn logical_and(&mut self) -> Result<i64> {
        let mut value = self.bit_or()?;
        while self.op("&&") {
            let rhs = self.bit_or()?;
            value = ((value != 0) && (rhs != 0)) as i64;
        }
        Ok(value)
    }

    fn bit_or(&mut self) -> Result<i64> {
        let mut value = self.bit_xor()?;
        while self.op("|") { value |= self.bit_xor()?; }
        Ok(value)
    }

    fn bit_xor(&mut self) -> Result<i64> {
        let mut value = self.bit_and()?;
        while self.op("^") { value ^= self.bit_and()?; }
        Ok(value)
    }

    fn bit_and(&mut self) -> Result<i64> {
        let mut value = self.equality()?;
        while self.op("&") { value &= self.equality()?; }
        Ok(value)
    }

    fn equality(&mut self) -> Result<i64> {
        let mut value = self.relational()?;
        loop {
            if self.op("==") { value = (value == self.relational()?) as i64; }
            else if self.op("!=") { value = (value != self.relational()?) as i64; }
            else { break; }
        }
        Ok(value)
    }

    fn relational(&mut self) -> Result<i64> {
        let mut value = self.shift()?;
        loop {
            if self.op("<=") { value = (value <= self.shift()?) as i64; }
            else if self.op(">=") { value = (value >= self.shift()?) as i64; }
            else if self.op("<") { value = (value < self.shift()?) as i64; }
            else if self.op(">") { value = (value > self.shift()?) as i64; }
            else { break; }
        }
        Ok(value)
    }

    fn shift(&mut self) -> Result<i64> {
        let mut value = self.additive()?;
        loop {
            if self.op("<<") { value <<= self.additive()?; }
            else if self.op(">>") { value >>= self.additive()?; }
            else { break; }
        }
        Ok(value)
    }

    fn additive(&mut self) -> Result<i64> {
        let mut value = self.multiplicative()?;
        loop {
            if self.op("+") { value = value.wrapping_add(self.multiplicative()?); }
            else if self.op("-") { value = value.wrapping_sub(self.multiplicative()?); }
            else { break; }
        }
        Ok(value)
    }

    fn multiplicative(&mut self) -> Result<i64> {
        let mut value = self.power()?;
        loop {
            if self.op("*") { value = value.wrapping_mul(self.power()?); }
            else if self.op("/") {
                let rhs = self.power()?;
                if rhs == 0 { bail!("división por cero"); }
                value /= rhs;
            } else if self.op("%") {
                let rhs = self.power()?;
                if rhs == 0 { bail!("división por cero"); }
                value %= rhs;
            } else { break; }
        }
        Ok(value)
    }

    fn power(&mut self) -> Result<i64> {
        let value = self.unary()?;
        if self.op("**") {
            let exponent = self.power()?;
            if exponent < 0 { return Ok(0); }
            Ok(value.wrapping_pow(exponent as u32))
        } else {
            Ok(value)
        }
    }

    fn unary(&mut self) -> Result<i64> {
        if self.op("+") { return self.unary(); }
        if self.op("-") { return Ok(-self.unary()?); }
        if self.op("!") { return Ok((self.unary()? == 0) as i64); }
        if self.op("~") { return Ok(!self.unary()?); }
        self.primary()
    }

    fn primary(&mut self) -> Result<i64> {
        match self.take() {
            ArithmeticToken::Number(value) => Ok(value),
            ArithmeticToken::Ident(name) => {
                if let Some(open) = name.find('[')
                    && name.ends_with(']')
                {
                    let base = &name[..open];
                    let subscript = &name[open + 1..name.len() - 1];
                    if self.env.assoc_arrays.contains_key(base) {
                        return Ok(self.env.get(&name).parse::<i64>().unwrap_or(0));
                    }
                    let index = eval_arithmetic(subscript, self.env)?;
                    let reference = format!("{base}[{index}]");
                    Ok(self.env.get(&reference).parse::<i64>().unwrap_or(0))
                } else {
                    Ok(self.env.get(&name).parse::<i64>().unwrap_or(0))
                }
            },
            ArithmeticToken::LParen => {
                let value = self.comma()?;
                if !matches!(self.take(), ArithmeticToken::RParen) { bail!("paréntesis aritmético sin cerrar"); }
                Ok(value)
            }
            token => bail!("expresión aritmética inválida: {token:?}"),
        }
    }
}

fn eval_arithmetic(expression: &str, env: &ShellEnvironment) -> Result<i64> {
    ArithmeticParser::new(expression, env)?.parse()
}


fn parse_array_entry(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix('[')?;
    let close = rest.find(']')?;
    let key = rest[..close].to_owned();
    let tail = rest.get(close + 1..)?;
    let item = tail.strip_prefix('=')?.to_owned();
    Some((strip_outer_quotes(&key), strip_outer_quotes(&item)))
}

fn split_shell_words_relaxed(input: &str) -> Result<Vec<String>> {
    let tokens = super::lexer::lex(input)?;
    Ok(tokens.into_iter().filter_map(|token| {
        if let super::lexer::Token::Word(word) = token { Some(word) } else { None }
    }).collect())
}

fn file_mtime(path: &Path) -> std::time::SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

fn is_arithmetic_lvalue(name: &str) -> bool {
    let base = name.split('[').next().unwrap_or(name);
    is_variable_name(base)
}

fn split_arithmetic_top_level(expression: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in expression.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if ch == separator && depth == 0 => {
                parts.push(expression[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(expression[start..].trim());
    parts
}

fn find_arithmetic_assignment(expression: &str) -> Option<(&str, &str, &str)> {
    let operators = ["<<=", ">>=", "**=", "+=", "-=", "*=", "/=", "%=", "&=", "^=", "|=", "="];
    let bytes = expression.as_bytes();
    let mut depth = 0i32;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'(' => { depth += 1; index += 1; continue; }
            b')' => { depth -= 1; index += 1; continue; }
            _ => {}
        }
        if depth == 0 {
            for operator in operators {
                if expression[index..].starts_with(operator) {
                    if operator == "=" {
                        let previous = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
                        let next = bytes.get(index + 1).copied();
                        if matches!(previous, Some(b'=' | b'!' | b'<' | b'>')) || next == Some(b'=') {
                            continue;
                        }
                    }
                    let name = expression[..index].trim();
                    if !is_arithmetic_lvalue(name) { continue; }
                    let rhs = expression[index + operator.len()..].trim();
                    return Some((name, operator, rhs));
                }
            }
        }
        index += 1;
    }
    None
}


fn render_redirect(redirect: &super::ast::Redirect) -> String {
    let op = match redirect.kind {
        RedirectKind::Read => "<",
        RedirectKind::Write => ">",
        RedirectKind::Append => ">>",
        RedirectKind::DupInput => "<&",
        RedirectKind::DupOutput => ">&",
        RedirectKind::HereString => "<<<",
        RedirectKind::ReadWrite => "<>",
        RedirectKind::Clobber => ">|",
        RedirectKind::BothWrite => "&>",
        RedirectKind::BothAppend => "&>>",
    };
    if matches!(redirect.kind, RedirectKind::BothWrite | RedirectKind::BothAppend) {
        format!("{op} {}", shell_quote(&redirect.target))
    } else {
        let default_fd = match redirect.kind {
            RedirectKind::Read | RedirectKind::ReadWrite | RedirectKind::HereString => 0,
            _ => 1,
        };
        let fd = if redirect.fd == default_fd { String::new() } else { redirect.fd.to_string() };
        format!("{fd}{op} {}", shell_quote(&redirect.target))
    }
}

fn render_ast(node: &AstNode) -> String {
    match node {
        AstNode::Empty => String::new(),
        AstNode::Sequence(nodes) => nodes.iter().map(render_ast).collect::<Vec<_>>().join("; "),
        AstNode::And(left,right) => format!("{} && {}", render_ast(left), render_ast(right)),
        AstNode::Or(left,right) => format!("{} || {}", render_ast(left), render_ast(right)),
        AstNode::Pipeline { parts, stderr_to_pipe } => {
            let mut rendered = String::new();
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    rendered.push_str(if stderr_to_pipe.get(index - 1).copied().unwrap_or(false) { " |& " } else { " | " });
                }
                rendered.push_str(&render_ast(part));
            }
            rendered
        }
        AstNode::Time { body, posix } => format!("time {}{}", if *posix { "-p " } else { "" }, render_ast(body)),
        AstNode::Coproc { name, body } => match name {
            Some(name) => format!("coproc {name} {}", render_ast(body)),
            None => format!("coproc {}", render_ast(body)),
        },
        AstNode::Negate(body) => format!("! {}", render_ast(body)),
        AstNode::Background(body) => format!("{} &", render_ast(body)),
        AstNode::Simple(command) => {
            let mut parts = command.words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>();
            parts.extend(command.redirects.iter().map(render_redirect));
            parts.join(" ")
        }
        AstNode::ArrayAssign { name, words } => {
            format!("{name}=({})", words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" "))
        }
        AstNode::If { condition, then_branch, else_branch } => {
            let mut value = format!("if {}; then {}", render_ast(condition), render_ast(then_branch));
            if let Some(branch) = else_branch {
                value.push_str(&format!("; else {}", render_ast(branch)));
            }
            value.push_str("; fi");
            value
        }
        AstNode::For { name, words, body } => {
            let words = words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" ");
            format!("for {name} in {words}; do {}; done", render_ast(body))
        }
        AstNode::ArithmeticFor { init, condition, update, body } => {
            format!("for (( {init}; {condition}; {update} )); do {}; done", render_ast(body))
        }
        AstNode::Select { name, words, body } => {
            let words = words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" ");
            format!("select {name} in {words}; do {}; done", render_ast(body))
        }
        AstNode::While { condition, body, until } => {
            format!("{} {}; do {}; done", if *until { "until" } else { "while" }, render_ast(condition), render_ast(body))
        }
        AstNode::Case { word, arms } => {
            let mut value = format!("case {} in ", shell_quote(word));
            for arm in arms {
                value.push_str(&arm.patterns.join("|"));
                value.push_str(") ");
                value.push_str(&render_ast(&arm.body));
                value.push(' ');
                value.push_str(match arm.terminator {
                    CaseTerminator::Break => ";;",
                    CaseTerminator::Fallthrough => ";&",
                    CaseTerminator::ContinueMatching => ";;&",
                });
                value.push(' ');
            }
            value.push_str("esac");
            value
        }
        AstNode::Conditional(items) => format!("[[ {} ]]", items.join(" ")),
        AstNode::ArithmeticCommand(expression) => format!("(( {expression} ))"),
        AstNode::FunctionDef { name, body } => format!("{name}() {{ {}; }}", render_ast(body)),
        AstNode::Group(body) => format!("{{ {}; }}", render_ast(body)),
        AstNode::Subshell(body) => format!("( {} )", render_ast(body)),
    }
}


fn bash_special_builtin_names() -> &'static [&'static str] {
    &[":", ".", "break", "continue", "eval", "exec", "exit", "export",
      "readonly", "return", "set", "shift", "trap", "unset"]
}

fn bash_builtin_help(name: &str) -> (&'static str, &'static str) {
    match name {
        ":" => (":", "No hace nada y devuelve estado cero."),
        "." | "source" => ("source ARCHIVO [ARGS]", "Ejecuta ARCHIVO en el contexto de la shell actual."),
        "[" | "test" => ("test EXPRESIÓN", "Evalúa una expresión condicional."),
        "alias" => ("alias [NOMBRE[=VALOR] ...]", "Define o muestra alias."),
        "bg" => ("bg [JOB ...]", "Continúa jobs en segundo plano."),
        "bind" => ("bind [OPCIONES] [SECUENCIA:FUNCIÓN]", "Configura edición de línea."),
        "break" => ("break [N]", "Sale de bucles."),
        "builtin" => ("builtin BUILTIN [ARGS]", "Ejecuta un builtin ignorando funciones."),
        "caller" => ("caller [N]", "Muestra un frame de la pila de llamadas."),
        "cd" => ("cd [-L|-P] [DIR]", "Cambia el directorio actual."),
        "command" => ("command [-pVv] COMANDO [ARGS]", "Ejecuta o describe un comando sin funciones."),
        "compgen" => ("compgen [OPCIONES] [PALABRA]", "Genera candidatos de completion."),
        "complete" => ("complete [OPCIONES] [NOMBRE ...]", "Define completion programable."),
        "compopt" => ("compopt [-o OPCIÓN] [+o OPCIÓN] [NOMBRE ...]", "Modifica opciones de completion."),
        "continue" => ("continue [N]", "Continúa la siguiente iteración de un bucle."),
        "declare" | "typeset" => ("declare [OPCIONES] [NOMBRE[=VALOR] ...]", "Declara variables y atributos."),
        "dirs" => ("dirs [-clpv] [+N|-N]", "Muestra la pila de directorios."),
        "disown" => ("disown [-ar] [-h] [JOB ...]", "Elimina jobs de la tabla de jobs."),
        "echo" => ("echo [-neE] [ARG ...]", "Escribe argumentos."),
        "enable" => ("enable [-a] [-dnps] [NOMBRE ...]", "Activa o desactiva builtins."),
        "eval" => ("eval [ARG ...]", "Evalúa argumentos como código shell."),
        "exec" => ("exec [-cl] [-a NOMBRE] [COMANDO [ARGS]]", "Reemplaza la shell por un comando."),
        "exit" => ("exit [N]", "Sale de la shell."),
        "export" => ("export [-fn] [NOMBRE[=VALOR] ...]", "Marca variables para exportación."),
        "false" => ("false", "Devuelve estado distinto de cero."),
        "fc" => ("fc [-e EDITOR] [-lnr] [PRIMERO] [ÚLTIMO]", "Lista, edita o reejecuta historial."),
        "fg" => ("fg [JOB]", "Trae un job al primer plano."),
        "getopts" => ("getopts OPTSTRING NOMBRE [ARGS]", "Analiza opciones posicionales."),
        "hash" => ("hash [-lr] [-p RUTA] [-dt] [NOMBRE ...]", "Gestiona la tabla hash de comandos."),
        "help" => ("help [-dms] [PATRÓN ...]", "Muestra ayuda de builtins Bash."),
        "history" => ("history [OPCIONES] [N]", "Muestra o modifica el historial."),
        "jobs" => ("jobs [-lnprs] [JOB ...]", "Lista jobs."),
        "kill" => ("kill [-s SEÑAL | -n SEÑAL | -SEÑAL] PID|%JOB ...", "Envía una señal a procesos o jobs."),
        "let" => ("let ARG ...", "Evalúa expresiones aritméticas."),
        "local" => ("local [OPCIONES] NOMBRE[=VALOR] ...", "Declara variables locales."),
        "logout" => ("logout [N]", "Sale de una shell de login."),
        "mapfile" | "readarray" => ("mapfile [OPCIONES] [ARRAY]", "Lee registros en un array indexado."),
        "popd" => ("popd [-n] [+N|-N]", "Elimina una entrada de la pila de directorios."),
        "printf" => ("printf [-v VAR] FORMATO [ARG ...]", "Imprime usando formato Bash."),
        "pushd" => ("pushd [-n] [DIR|+N|-N]", "Añade o rota la pila de directorios."),
        "pwd" => ("pwd [-LP]", "Muestra el directorio actual."),
        "read" => ("read [OPCIONES] [NOMBRE ...]", "Lee una línea o registro."),
        "readonly" => ("readonly [-aAf] [NOMBRE[=VALOR] ...]", "Marca variables o funciones como solo lectura."),
        "return" => ("return [N]", "Retorna de una función o archivo sourced."),
        "set" => ("set [-abefhkmnptuvxBCEHPT] [-o OPCIÓN] [--] [ARG ...]", "Configura opciones y parámetros posicionales."),
        "shift" => ("shift [N]", "Desplaza parámetros posicionales."),
        "shopt" => ("shopt [-pqsu] [-o] [OPCIÓN ...]", "Configura opciones adicionales Bash."),
        "suspend" => ("suspend [-f]", "Suspende una shell interactiva cuando el host lo permite."),
        "times" => ("times", "Muestra tiempos de CPU de shell y procesos hijos."),
        "trap" => ("trap [-lp] [[ARG] SEÑAL ...]", "Configura acciones para señales y pseudo-señales."),
        "true" => ("true", "Devuelve estado cero."),
        "type" => ("type [-afptP] NOMBRE ...", "Describe cómo se resolvería un nombre."),
        "ulimit" => ("ulimit [-SHabcdefiklmnpqrstuvxPRT] [LÍMITE]", "Consulta o establece límites de recursos."),
        "umask" => ("umask [-p] [-S] [MÁSCARA]", "Muestra o establece la máscara de creación."),
        "unalias" => ("unalias [-a] NOMBRE ...", "Elimina alias."),
        "unset" => ("unset [-fnv] NOMBRE ...", "Elimina variables o funciones."),
        "wait" => ("wait [-fn] [-p VAR] [ID ...]", "Espera procesos o jobs."),
        _ => (name, "Builtin Bash."),
    }
}

fn bash_builtin_names() -> &'static [&'static str] {
    &[
        ":", ".", "[", "alias", "bg", "bind", "break", "builtin", "caller", "cd",
        "command", "compgen", "complete", "compopt", "continue", "declare", "dirs",
        "disown", "echo", "enable", "eval", "exec", "exit", "export", "false", "fc",
        "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let", "local",
        "logout", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray",
        "readonly", "return", "set", "shift", "shopt", "source", "suspend", "test",
        "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias",
        "unset", "wait",
    ]
}

fn bash_keywords() -> &'static [&'static str] {
    &[
        "!", "[[", "]]", "case", "coproc", "do", "done", "elif", "else", "esac",
        "fi", "for", "function", "if", "in", "select", "then", "time", "until",
        "while", "{", "}",
    ]
}

fn host_completion_candidates(prefix: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    if let Ok(host) = std::env::var("COMPUTERNAME") {
        hosts.push(host);
    }

    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let path = PathBuf::from(root).join("System32").join("drivers").join("etc").join("hosts");
        if let Ok(contents) = fs::read_to_string(path) {
            for line in contents.lines() {
                let content = line.split('#').next().unwrap_or("").trim();
                if content.is_empty() { continue; }
                let mut fields = content.split_whitespace();
                let _address = fields.next();
                hosts.extend(fields.map(str::to_owned));
            }
        }
    }

    let needle = prefix.to_lowercase();
    hosts.retain(|host| host.to_lowercase().starts_with(&needle));
    hosts.sort();
    hosts.dedup();
    hosts
}

fn completion_files(cwd: &Path, prefix: &str, directories_only: bool) -> Vec<String> {
    let typed = PathBuf::from(prefix);
    let parent = typed.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = typed.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let directory = if parent.is_absolute() { parent.to_path_buf() } else { cwd.join(parent) };
    let mut values = Vec::new();

    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            if directories_only && !file_type.is_dir() { continue; }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(stem) { continue; }
            let mut value = if parent == Path::new(".") {
                name
            } else {
                parent.join(name).to_string_lossy().into_owned()
            };
            if file_type.is_dir() {
                value.push(std::path::MAIN_SEPARATOR);
            }
            values.push(value);
        }
    }
    values.sort();
    values
}

fn path_commands(path_value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let directories: Vec<PathBuf> = if path_value.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        std::env::split_paths(path_value)
            .map(|path| if path.as_os_str().is_empty() { PathBuf::from(".") } else { path })
            .collect()
    };
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else { continue };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            if !file_type.is_file() { continue; }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
            #[cfg(windows)]
            {
                let lower = name.to_ascii_lowercase();
                if !(lower.ends_with(".exe")
                    || lower.ends_with(".com")
                    || lower.ends_with(".bat")
                    || lower.ends_with(".cmd")
                    || lower.ends_with(".sh"))
                {
                    continue;
                }
                values.push(
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(name)
                        .to_owned()
                );
            }
            #[cfg(not(windows))]
            values.push(name.to_owned());
        }
    }
    values.sort();
    values.dedup();
    values
}


fn resolve_history_event(chars: &[char], history: &[String]) -> Result<(String, usize)> {
    if chars.first() != Some(&'!') { bail!("expansión de historial inválida"); }
    if chars.get(1) == Some(&'!') { return Ok((history.last().cloned().unwrap_or_default(), 2)); }
    if chars.get(1) == Some(&'-') {
        let mut end = 2usize;
        while end < chars.len() && chars[end].is_ascii_digit() { end += 1; }
        let offset = chars[2..end].iter().collect::<String>().parse::<usize>().unwrap_or(0);
        if offset == 0 || offset > history.len() { bail!("evento de historial no encontrado"); }
        return Ok((history[history.len() - offset].clone(), end));
    }
    if chars.get(1).is_some_and(|ch| ch.is_ascii_digit()) {
        let mut end = 1usize;
        while end < chars.len() && chars[end].is_ascii_digit() { end += 1; }
        let number = chars[1..end].iter().collect::<String>().parse::<usize>().unwrap_or(0);
        if number == 0 || number > history.len() { bail!("evento de historial no encontrado"); }
        return Ok((history[number - 1].clone(), end));
    }
    if chars.get(1) == Some(&'?') {
        let mut end = 2usize;
        while end < chars.len() && chars[end] != '?' { end += 1; }
        if end >= chars.len() { bail!("evento de historial sin '?' final"); }
        let needle: String = chars[2..end].iter().collect();
        let event = history.iter().rev().find(|line| line.contains(&needle))
            .cloned().ok_or_else(|| anyhow!("evento de historial no encontrado: {needle}"))?;
        return Ok((event, end + 1));
    }
    if chars.get(1) == Some(&'#') { return Ok((String::new(), 2)); }
    let mut end = 1usize;
    while end < chars.len() && !chars[end].is_whitespace()
        && !matches!(chars[end], ':' | ';' | '&' | '|' | '(' | ')' | '<' | '>') { end += 1; }
    let prefix: String = chars[1..end].iter().collect();
    if prefix.is_empty() { bail!("designador de historial vacío"); }
    let event = history.iter().rev().find(|line| line.starts_with(&prefix))
        .cloned().ok_or_else(|| anyhow!("evento de historial no encontrado: {prefix}"))?;
    Ok((event, end))
}

fn apply_history_modifiers(chars: &[char], event: &str) -> Result<(String, usize)> {
    let mut value = event.to_owned();
    let mut i = 0usize;
    while i < chars.len() && chars[i] == ':' {
        i += 1;
        if i >= chars.len() { break; }
        match chars[i] {
            '^' | '$' | '*' | '0'..='9' => {
                let words = split_shell_words_relaxed(&value)
                    .unwrap_or_else(|_| value.split_whitespace().map(str::to_owned).collect());
                if words.is_empty() { value.clear(); i += 1; continue; }
                match chars[i] {
                    '^' => { value = words.get(1).cloned().unwrap_or_default(); i += 1; }
                    '$' => { value = words.last().cloned().unwrap_or_default(); i += 1; }
                    '*' => { value = words.get(1..).unwrap_or(&[]).join(" "); i += 1; }
                    _ => {
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                        let first = chars[start..i].iter().collect::<String>().parse::<usize>().unwrap_or(0);
                        let mut last = first;
                        if i < chars.len() && chars[i] == '-' {
                            i += 1;
                            let range_start = i;
                            while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                            last = if range_start == i { words.len().saturating_sub(1) } else {
                                chars[range_start..i].iter().collect::<String>().parse::<usize>().unwrap_or(first)
                            };
                        }
                        value = if first < words.len() {
                            words[first..=last.min(words.len() - 1)].join(" ")
                        } else { String::new() };
                    }
                }
            }
            'h' => { value = Path::new(&value).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(); i += 1; }
            't' => { value = Path::new(&value).file_name().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(); i += 1; }
            'r' => { value = Path::new(&value).with_extension("").to_string_lossy().into_owned(); i += 1; }
            'e' => { value = Path::new(&value).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default(); i += 1; }
            'q' | 'x' => { value = shell_quote(&value); i += 1; }
            's' => {
                i += 1; if i >= chars.len() { break; }
                let delimiter = chars[i]; i += 1;
                let from_start = i; while i < chars.len() && chars[i] != delimiter { i += 1; }
                if i >= chars.len() { bail!("modificador :s incompleto"); }
                let from: String = chars[from_start..i].iter().collect(); i += 1;
                let to_start = i; while i < chars.len() && chars[i] != delimiter { i += 1; }
                let to: String = chars[to_start..i].iter().collect(); if i < chars.len() { i += 1; }
                value = value.replacen(&from, &to, 1);
            }
            'g' if chars.get(i + 1) == Some(&'s') => {
                i += 2; if i >= chars.len() { break; }
                let delimiter = chars[i]; i += 1;
                let from_start = i; while i < chars.len() && chars[i] != delimiter { i += 1; }
                if i >= chars.len() { bail!("modificador :gs incompleto"); }
                let from: String = chars[from_start..i].iter().collect(); i += 1;
                let to_start = i; while i < chars.len() && chars[i] != delimiter { i += 1; }
                let to: String = chars[to_start..i].iter().collect(); if i < chars.len() { i += 1; }
                value = value.replace(&from, &to);
            }
            _ => break,
        }
    }
    Ok((value, i))
}


fn preserve_interactive_hashes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    let mut word_start = true;

    for ch in input.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            word_start = false;
            continue;
        }
        if ch == '\\' && !single {
            out.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' && !double {
            single = !single;
            out.push(ch);
            word_start = false;
            continue;
        }
        if ch == '"' && !single {
            double = !double;
            out.push(ch);
            word_start = false;
            continue;
        }
        if !single && !double && ch == '#' && word_start {
            out.push('\\');
            out.push('#');
            word_start = false;
            continue;
        }
        out.push(ch);
        word_start = !single && !double && (ch.is_whitespace() || matches!(ch, ';' | '&' | '|'));
    }
    out
}

fn spelling_distance_one(left: &str, right: &str) -> bool {
    if left.eq_ignore_ascii_case(right) { return true; }
    let a = left.to_lowercase().chars().collect::<Vec<_>>();
    let b = right.to_lowercase().chars().collect::<Vec<_>>();
    if a.len().abs_diff(b.len()) > 1 { return false; }

    if a.len() == b.len() {
        let differences = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        if differences == 1 { return true; }
        for index in 0..a.len().saturating_sub(1) {
            if a[index] != b[index]
                && a[index] == b[index + 1]
                && a[index + 1] == b[index]
                && a[..index] == b[..index]
                && a[index + 2..] == b[index + 2..]
            {
                return true;
            }
        }
        return false;
    }

    let (short, long) = if a.len() < b.len() { (&a, &b) } else { (&b, &a) };
    let mut i = 0usize;
    let mut j = 0usize;
    let mut skipped = false;
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
        } else if skipped {
            return false;
        } else {
            skipped = true;
            j += 1;
        }
    }
    true
}

fn bash_shell_options() -> &'static [&'static str] {
    &[
        "allexport", "braceexpand", "emacs", "errexit", "errtrace", "functrace",
        "hashall", "histexpand", "history", "ignoreeof", "interactive-comments",
        "keyword", "monitor", "noclobber", "noexec", "noglob", "nolog", "notify",
        "nounset", "onecmd", "physical", "pipefail", "posix", "privileged",
        "verbose", "vi", "xtrace",
    ]
}

fn bash_shopt_options() -> &'static [&'static str] {
    &[
        "array_expand_once", "assoc_expand_once", "autocd", "bash_source_fullpath", "cdable_vars", "cdspell",
        "checkhash", "checkjobs", "checkwinsize", "cmdhist", "compat31", "compat32",
        "compat40", "compat41", "compat42", "compat43", "compat44", "compat50",
        "compat51", "compat52", "compat53", "complete_fullquote", "direxpand",
        "dirspell", "dotglob", "execfail", "expand_aliases", "extdebug", "extglob",
        "extquote", "failglob", "force_fignore", "globasciiranges", "globskipdots",
        "globstar", "gnu_errfmt", "histappend", "histreedit", "histverify",
        "hostcomplete", "huponexit", "inherit_errexit", "interactive_comments",
        "lastpipe", "lithist", "localvar_inherit", "localvar_unset", "login_shell",
        "mailwarn", "no_empty_cmd_completion", "nocaseglob", "nocasematch",
        "noexpand_translation", "nullglob", "patsub_replacement", "progcomp", "progcomp_alias",
        "promptvars", "restricted_shell", "shift_verbose", "sourcepath",
        "varredir_close", "xpg_echo",
    ]
}


fn normalize_glob_path(value: &str) -> String {
    value.replace('\\', "/")
}

fn contains_extglob(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.windows(2).any(|pair| matches!(pair[0], '?' | '*' | '+' | '@' | '!') && pair[1] == '(')
}

fn extglob_broad_pattern(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if matches!(chars[index], '?' | '*' | '+' | '@' | '!')
            && chars.get(index + 1) == Some(&'(')
        {
            let mut depth = 1usize;
            let mut end = index + 2;
            while end < chars.len() && depth > 0 {
                match chars[end] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            out.push('*');
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn split_extglob_alternatives(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        match ch {
            '(' => { depth += 1; current.push(ch); }
            ')' => { depth = depth.saturating_sub(1); current.push(ch); }
            '|' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

fn bash_glob_fragment(pattern: &str) -> Result<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;

    while index < chars.len() {
        if matches!(chars[index], '?' | '*' | '+' | '@' | '!')
            && chars.get(index + 1) == Some(&'(')
        {
            let operator = chars[index];
            let mut depth = 1usize;
            let mut end = index + 2;
            while end < chars.len() && depth > 0 {
                match chars[end] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            if depth != 0 { bail!("extglob sin cerrar"); }
            let body: String = chars[index + 2..end - 1].iter().collect();
            let alternatives = split_extglob_alternatives(&body)
                .into_iter()
                .map(|part| bash_glob_fragment(&part))
                .collect::<Result<Vec<_>>>()?
                .join("|");
            match operator {
                '@' => out.push_str(&format!("(?:{alternatives})")),
                '?' => out.push_str(&format!("(?:{alternatives})?")),
                '+' => out.push_str(&format!("(?:{alternatives})+")),
                '*' => out.push_str(&format!("(?:{alternatives})*")),
                '!' => out.push_str("[^/]*"),
                _ => {}
            }
            index = end;
            continue;
        }

        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                while chars.get(index + 1) == Some(&'*') { index += 1; }
                out.push_str(".*");
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let start = index;
                index += 1;
                while index < chars.len() && chars[index] != ']' { index += 1; }
                if index < chars.len() {
                    let mut class: String = chars[start + 1..index].iter().collect();
                    if class.starts_with('!') { class.replace_range(..1, "^"); }
                    out.push('[');
                    out.push_str(&class);
                    out.push(']');
                } else {
                    out.push_str("\\[");
                    index = start;
                }
            }
            '/' => out.push('/'),
            ch => {
                if matches!(ch, '.' | '^' | '$' | '+' | '(' | ')' | '{' | '}' | '|' | '\\') {
                    out.push('\\');
                }
                out.push(ch);
            }
        }
        index += 1;
    }
    Ok(out)
}

fn bash_glob_regex(pattern: &str, nocase: bool) -> Result<regex::Regex> {
    let fragment = bash_glob_fragment(pattern)?;
    Ok(regex::Regex::new(&if nocase {
        format!("(?i)^{fragment}$")
    } else {
        format!("^{fragment}$")
    })?)
}

fn negative_extglob_rejects(pattern: &str, candidate: &str, nocase: bool) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let candidate_parts: Vec<&str> = candidate.split('/').collect();
    if pattern_parts.len() != candidate_parts.len() { return false; }

    for (pattern_part, candidate_part) in pattern_parts.into_iter().zip(candidate_parts) {
        if pattern_part.starts_with("!(") && pattern_part.ends_with(')') {
            for alternative in split_extglob_alternatives(&pattern_part[2..pattern_part.len() - 1]) {
                if bash_glob_regex(&alternative, nocase).is_ok_and(|regex| regex.is_match(candidate_part)) {
                    return true;
                }
            }
        }
    }
    false
}

fn natural_numeric_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = left.as_bytes();
    let b = right.as_bytes();
    let mut i = 0usize;
    let mut j = 0usize;

    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let ai = i;
            let bj = j;
            while i < a.len() && a[i].is_ascii_digit() { i += 1; }
            while j < b.len() && b[j].is_ascii_digit() { j += 1; }
            let an = &left[ai..i];
            let bn = &right[bj..j];
            let at = an.trim_start_matches('0');
            let bt = bn.trim_start_matches('0');
            let at = if at.is_empty() { "0" } else { at };
            let bt = if bt.is_empty() { "0" } else { bt };
            match at.len().cmp(&bt.len())
                .then_with(|| at.cmp(bt))
                .then_with(|| an.len().cmp(&bn.len()))
            {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        } else {
            match a[i].cmp(&b[j]) {
                Ordering::Equal => { i += 1; j += 1; }
                ordering => return ordering,
            }
        }
    }
    a.len().cmp(&b.len())
}

fn sort_glob_results(values: &mut [String], sort: &str, cwd: &Path) {
    let mut mode = sort.trim();
    let mut reverse = false;
    if let Some(rest) = mode.strip_prefix('-') {
        reverse = true;
        mode = rest;
    }
    if mode.is_empty() { mode = "name"; }
    if mode == "none" {
        if reverse { values.reverse(); }
        return;
    }

    values.sort_by(|left, right| {
        let left_path = {
            let path = PathBuf::from(left);
            if path.is_absolute() { path } else { cwd.join(path) }
        };
        let right_path = {
            let path = PathBuf::from(right);
            if path.is_absolute() { path } else { cwd.join(path) }
        };
        let left_meta = fs::metadata(left_path).ok();
        let right_meta = fs::metadata(right_path).ok();

        let ordering = match mode {
            "size" => left_meta.as_ref().map(|m| m.len()).cmp(&right_meta.as_ref().map(|m| m.len())),
            "blocks" => left_meta.as_ref().map(|m| (m.len() + 511) / 512)
                .cmp(&right_meta.as_ref().map(|m| (m.len() + 511) / 512)),
            "mtime" => left_meta.as_ref().and_then(|m| m.modified().ok())
                .cmp(&right_meta.as_ref().and_then(|m| m.modified().ok())),
            "atime" => left_meta.as_ref().and_then(|m| m.accessed().ok())
                .cmp(&right_meta.as_ref().and_then(|m| m.accessed().ok())),
            "ctime" => left_meta.as_ref().and_then(|m| m.created().ok())
                .cmp(&right_meta.as_ref().and_then(|m| m.created().ok())),
            "numeric" => natural_numeric_cmp(left, right),
            _ => left.cmp(right),
        }.then_with(|| left.cmp(right));

        if reverse { ordering.reverse() } else { ordering }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullHost;

    impl ShellCommandHost for NullHost {
        fn execute_builtin(
            &self, _name: &str, _args: &[String], _cwd: &Path, _stdin: Option<&[u8]>,
        ) -> Result<Option<ExecutionResult>> {
            Ok(None)
        }

        fn execute_external(
            &self, program: &str, _args: &[String], _cwd: &Path,
            _env: &HashMap<String, String>, _stdin: Option<&[u8]>,
        ) -> Result<ExecutionResult> {
            Ok(ExecutionResult::from_parts(format!("external:{program}\n"), String::new(), 0))
        }
    }

    #[test]
    fn state_functions_and_subshells_are_native() {
        let mut shell = Interpreter::new(Box::new(NullHost));
        shell.execute_text("x=42").unwrap();
        assert_eq!(shell.env.get("x"), "42");
        shell.execute_text("f() { x=99; }").unwrap();
        shell.execute_text("f").unwrap();
        assert_eq!(shell.env.get("x"), "99");
        shell.execute_text("(x=7)").unwrap();
        assert_eq!(shell.env.get("x"), "99");
    }

    #[test]
    fn native_expansions_work_without_an_external_shell() {
        let mut shell = Interpreter::new(Box::new(NullHost));
        assert_eq!(shell.expand_scalar("$((20+22))").unwrap(), "42");
        assert_eq!(shell.expand_scalar("${missing:-fallback}").unwrap(), "fallback");
    }


    #[test]
    fn case_conditional_and_arithmetic_commands_work() {
        let mut shell = Interpreter::new(Box::new(NullHost));

        shell.execute_text("x=beta").unwrap();
        let case_result = shell
            .execute_text("case $x in alpha) echo no ;; beta|gamma) echo yes ;; *) echo fallback ;; esac")
            .unwrap();
        assert!(case_result.stdout.contains("external:echo"));

        assert_eq!(shell.execute_text("[[ -n $x && $x == beta ]]").unwrap().status, 0);
        assert_eq!(shell.execute_text("[[ $x == nope ]]").unwrap().status, 1);

        shell.execute_text("n=1").unwrap();
        assert_eq!(shell.execute_text("(( n += 2 ))").unwrap().status, 0);
        assert_eq!(shell.env.get("n"), "3");
        assert_eq!(shell.execute_text("(( n > 2 ))").unwrap().status, 0);
    }

    #[test]
    fn break_continue_return_and_local_are_native() {
        let mut shell = Interpreter::new(Box::new(NullHost));

        shell.execute_text("x=outer").unwrap();
        shell.execute_text("f() { local x=inner; return 7; x=never; }").unwrap();
        let result = shell.execute_text("f").unwrap();
        assert_eq!(result.status, 7);
        assert_eq!(shell.env.get("x"), "outer");

        shell.execute_text("count=0").unwrap();
        shell.execute_text("for i in 1 2 3 4; do (( count += 1 )); if [[ $i == 2 ]]; then continue; fi; if [[ $i == 3 ]]; then break; fi; done").unwrap();
        assert_eq!(shell.env.get("count"), "3");
    }

    struct XargsHost;

    impl ShellCommandHost for XargsHost {
        fn execute_builtin(
            &self, _name: &str, _args: &[String], _cwd: &Path, _stdin: Option<&[u8]>,
        ) -> Result<Option<ExecutionResult>> {
            Ok(None)
        }

        fn execute_external(
            &self, program: &str, args: &[String], _cwd: &Path,
            _env: &HashMap<String, String>, _stdin: Option<&[u8]>,
        ) -> Result<ExecutionResult> {
            if program == "echo" {
                Ok(ExecutionResult::from_parts(
                    format!("{}\n", args.join(" ")),
                    String::new(),
                    0,
                ))
            } else {
                Ok(ExecutionResult::from_parts(
                    String::new(),
                    format!("unknown:{program}\n"),
                    127,
                ))
            }
        }
    }

    #[test]
    fn xargs_consumes_pipeline_input() {
        let mut shell = Interpreter::new(Box::new(XargsHost));
        let ast = parse("xargs -n 1 echo").unwrap();
        let result = shell.execute(&ast, Some(b"uno dos tres\n")).unwrap();
        assert_eq!(result.stdout, "uno\ndos\ntres\n");
        assert_eq!(result.status, 0);
    }

}
