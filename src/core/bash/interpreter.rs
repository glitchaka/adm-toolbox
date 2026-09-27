use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
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
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub pid: u32,
    pub command: String,
    pub running: bool,
}

#[derive(Debug)]
struct ProcessSubstitution {
    path: PathBuf,
    command: Option<String>,
    stderr: String,
}

#[derive(Debug, Clone, Copy)]
struct ExpansionCheckpoint {
    stdout_len: usize,
    stderr_len: usize,
    statuses_len: usize,
    exits_len: usize,
    process_sub_len: usize,
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
    }
    pub fn success() -> Self {
        Self::from_parts(String::new(), String::new(), 0)
    }

    pub fn from_parts(stdout: String, stderr: String, status: i32) -> Self {
        Self { stdout, stderr, status, exit_requested: false, flow: FlowSignal::None }
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

    fn read_line_with_options(
        &self,
        prompt: &str,
        silent: bool,
        initial: &str,
        timeout: Option<Duration>,
        delimiter: Option<char>,
        max_chars: Option<usize>,
        exact_chars: bool,
    ) -> Result<Option<String>> {
        let _ = (initial, timeout, delimiter, max_chars, exact_chars);
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

    fn command_is_builtin(&self, _name: &str) -> bool { false }
    fn jobs(&self) -> Result<Vec<JobInfo>> { Ok(Vec::new()) }
    fn wait_job(&self, _pid: Option<u32>) -> Result<i32> { Ok(127) }
    fn wait_next_job(&self) -> Result<Option<(u32, i32)>> { Ok(None) }
    fn disown_job(&self, _pid: u32) -> Result<bool> { Ok(false) }
    fn shell_times(&self) -> Result<(Duration, Duration, Duration, Duration)> {
        Ok((Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO))
    }
}

pub struct Interpreter {
    pub env: ShellEnvironment,
    host: Box<dyn ShellCommandHost>,
    loop_depth: usize,
    source_depth: usize,
    command_hash: HashMap<String, String>,
    disabled_builtins: HashSet<String>,
    call_stack: Vec<(String, String)>,
    source_stack: Vec<String>,
    process_sub_counter: u64,
    pending_process_substitutions: Vec<ProcessSubstitution>,
    pending_expansion_stdout: String,
    pending_expansion_stderr: String,
    pending_substitution_statuses: Vec<i32>,
    pending_expansion_exits: Vec<i32>,
}

impl Interpreter {
    pub fn new(host: Box<dyn ShellCommandHost>) -> Self {
        Self {
            env: ShellEnvironment::new(),
            host,
            loop_depth: 0,
            source_depth: 0,
            command_hash: HashMap::new(),
            disabled_builtins: HashSet::new(),
            call_stack: Vec::new(),
            source_stack: Vec::new(),
            process_sub_counter: 0,
            pending_process_substitutions: Vec::new(),
            pending_expansion_stdout: String::new(),
            pending_expansion_stderr: String::new(),
            pending_substitution_statuses: Vec::new(),
            pending_expansion_exits: Vec::new(),
        }
    }

    pub fn execute_text(&mut self, input: &str) -> Result<ExecutionResult> {
        let (prepared, temporary) = self.prepare_heredocs(input)?;
        let node = parse(&prepared)?;

        let result = if self.env.option_enabled("noexec") {
            ExecutionResult::success()
        } else {
            self.execute(&node, None)?
        };

        for name in temporary {
            self.env.unset(&name);
        }

        if result.status != 0 {
            if let Some(action) = self.env.traps.get("ERR").cloned() {
                let mut trap_result = self.execute(&parse(&action)?, None)?;
                trap_result.status = result.status;
                return Ok(trap_result);
            }
        }

        Ok(result)
    }

    pub fn execute(&mut self, node: &AstNode, stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        if self.host.interrupted() { bail!("comando interrumpido"); }
        let result = match node {
            AstNode::Empty => ExecutionResult::success(),
            AstNode::Sequence(nodes) => {
                let mut last = ExecutionResult::success();
                for node in nodes {
                    last.append(self.execute(node, stdin)?);
                    if last.exit_requested || last.flow != FlowSignal::None { break; }
                    if last.status != 0 && self.env.option_enabled("errexit") { break; }
                }
                last
            }
            AstNode::And(left, right) => {
                let mut left = self.execute(left, stdin)?;
                if !left.exit_requested && left.flow == FlowSignal::None && left.status == 0 {
                    left.append(self.execute(right, stdin)?);
                }
                left
            }
            AstNode::Or(left, right) => {
                let mut left = self.execute(left, stdin)?;
                if !left.exit_requested && left.flow == FlowSignal::None && left.status != 0 {
                    left.append(self.execute(right, stdin)?);
                }
                left
            }
            AstNode::Pipeline { parts, stderr_to_pipe } => self.execute_pipeline(parts, stderr_to_pipe, stdin)?,
            AstNode::Time { body, posix } => {
                let started = std::time::Instant::now();
                let before = self.host.shell_times().unwrap_or((
                    Duration::ZERO,
                    Duration::ZERO,
                    Duration::ZERO,
                    Duration::ZERO,
                ));
                let mut result = self.execute(body, stdin)?;
                let real = started.elapsed();
                let after = self.host.shell_times().unwrap_or(before);
                let user = after.0.saturating_sub(before.0);
                let system = after.1.saturating_sub(before.1);

                fn seconds(value: Duration) -> f64 {
                    value.as_secs_f64()
                }

                let timing = if *posix {
                    format!(
                        "real {:.3}\nuser {:.3}\nsys {:.3}\n",
                        seconds(real),
                        seconds(user),
                        seconds(system),
                    )
                } else if let Some(format_string) = self.env.vars.get("TIMEFORMAT").cloned() {
                    let total_cpu = user + system;
                    let percent = if real.is_zero() {
                        0.0
                    } else {
                        total_cpu.as_secs_f64() * 100.0 / real.as_secs_f64()
                    };
                    let mut rendered = String::new();
                    let mut chars = format_string.chars().peekable();
                    while let Some(ch) = chars.next() {
                        if ch != '%' {
                            rendered.push(ch);
                            continue;
                        }
                        match chars.next() {
                            Some('%') => rendered.push('%'),
                            Some('R') => rendered.push_str(&format!("{:.3}", seconds(real))),
                            Some('U') => rendered.push_str(&format!("{:.3}", seconds(user))),
                            Some('S') => rendered.push_str(&format!("{:.3}", seconds(system))),
                            Some('P') => rendered.push_str(&format!("{percent:.2}")),
                            Some(other) => {
                                rendered.push('%');
                                rendered.push(other);
                            }
                            None => rendered.push('%'),
                        }
                    }
                    if rendered.is_empty() { rendered } else { format!("{rendered}\n") }
                } else {
                    format!(
                        "\nreal\t0m{:.3}s\nuser\t0m{:.3}s\nsys\t0m{:.3}s\n",
                        seconds(real),
                        seconds(user),
                        seconds(system),
                    )
                };
                result.stderr.push_str(&timing);
                result
            }
            AstNode::Negate(body) => {
                let mut result = self.execute(body, stdin)?;
                if !result.exit_requested {
                    result.status = if result.status == 0 { 1 } else { 0 };
                }
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
                    let mut normalized = if append {
                        self.env.array_values(&actual_name)
                    } else {
                        Vec::new()
                    };
                    for value in values {
                        if let Some((key, item)) = parse_array_entry(&value) {
                            if let Ok(index) = key.parse::<usize>() {
                                if normalized.len() <= index { normalized.resize(index + 1, String::new()); }
                                normalized[index] = item;
                            } else {
                                normalized.push(value);
                            }
                        } else {
                            normalized.push(value);
                        }
                    }
                    if self.env.set_array(actual_name.clone(), normalized) {
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
                let mut condition = self.execute(condition, stdin)?;
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
                    let condition_result = self.execute(condition, None)?;
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
                        shell_pattern_matches(
                            &pattern,
                            &value,
                            self.env.option_enabled("extglob"),
                            self.env.option_enabled("nocasematch"),
                        )
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
                self.env.functions.insert(name.clone(), (**body).clone());
                ExecutionResult::success()
            }
            AstNode::Group(body) => self.execute(body, stdin)?,
            AstNode::Subshell(body) => {
                let saved = self.env.clone();
                let result = self.execute(body, stdin);
                self.env = saved;
                let mut result = result?;
                result.exit_requested = false;
                result.flow = FlowSignal::None;
                result
            }
        };

        self.env.last_status = result.status;
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
            if command.redirects.is_empty()
                && command.words.iter().all(|word| !word.contains("<(") && !word.contains(">("))
            {
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
                let mut child_env = self.env.exported.clone();
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

        let pid = match pid {
            Some(pid) => pid,
            None => self.host.execute_shell_background(
                &render_ast(node),
                &self.env.cwd,
                &self.env.exported,
            )?,
        };

        self.env.last_background_pid = Some(pid);
        let job_number = self.host.jobs()?
            .iter()
            .position(|job| job.pid == pid)
            .map(|index| index + 1)
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
        let mut input = stdin.map(ToOwned::to_owned);
        let mut stderr = String::new();
        let mut last = ExecutionResult::success();
        let mut last_nonzero = 0;
        let mut statuses = Vec::new();

        for (index, part) in parts.iter().enumerate() {
            let saved = self.env.clone();
            let result = self.execute(part, input.as_deref());
            self.env = saved;
            last = result?;
            last.exit_requested = false;
            last.flow = FlowSignal::None;

            if last.status != 0 { last_nonzero = last.status; }
            statuses.push(last.status.to_string());

            if index + 1 < parts.len() {
                let mut piped = last.stdout.as_bytes().to_vec();
                if stderr_to_pipe.get(index).copied().unwrap_or(false) {
                    piped.extend_from_slice(last.stderr.as_bytes());
                } else {
                    stderr.push_str(&last.stderr);
                }
                input = Some(piped);
            } else {
                stderr.push_str(&last.stderr);
            }
        }

        self.env.set_array("PIPESTATUS", statuses);
        last.stderr = stderr;
        if self.env.option_enabled("pipefail") && last_nonzero != 0 {
            last.status = last_nonzero;
        }
        Ok(last)
    }

    fn execute_simple(&mut self, command: &SimpleCommand, stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let checkpoint = self.expansion_checkpoint();
        let mut local_stdin = stdin.map(ToOwned::to_owned);

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
                RedirectKind::HereString => {
                    let mut value = self.expand_scalar(&redirect.target)?;
                    value.push('\n');
                    local_stdin = Some(value.into_bytes());
                }
                _ => {}
            }
        }

        if command.words.is_empty() {
            return self.finish_simple_result(checkpoint, false, ExecutionResult::success());
        }

        let mut raw = command.words.clone();
        if let Some(alias) = self.env.aliases.get(&raw[0]).cloned() {
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
            self.env.set(name.to_owned(), value);
            index += 1;
        }

        if index == raw.len() {
            return self.finish_simple_result(checkpoint, false, ExecutionResult::success());
        }

        let words = self.expand_words(&raw[index..])?;
        if words.is_empty() {
            return self.finish_simple_result(checkpoint, false, ExecutionResult::success());
        }

        let name = words[0].clone();
        let args = &words[1..];

        let mut trace = String::new();
        if self.env.option_enabled("xtrace") {
            let ps4 = self.env.vars.get("PS4").cloned().unwrap_or_else(|| "+ ".to_owned());
            trace.push_str(&ps4);
            trace.push_str(&words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" "));
            trace.push('\n');
        }

        let mut result = if let Some(body) = self.env.functions.get(&name).cloned() {
            let saved = self.env.positional.clone();
            let source = self.source_stack.last()
                .cloned()
                .unwrap_or_else(|| self.env.script_name.clone());
            self.call_stack.push((name.clone(), source));
            self.sync_call_stack_vars();
            self.env.positional = args.to_vec();
            self.env.push_local_scope();
            let execution = self.execute(&body, local_stdin.as_deref());
            self.env.pop_local_scope();
            self.env.positional = saved;
            self.call_stack.pop();
            self.sync_call_stack_vars();
            let mut result = execution?;
            if result.flow == FlowSignal::Return {
                result.flow = FlowSignal::None;
            }
            result
        } else if !self.disabled_builtins.contains(&name) {
            if let Some(result) = self.shell_builtin(&name, args, local_stdin.as_deref())? {
                result
            } else if let Some(result) = self.host.execute_builtin(
                &name,
                args,
                &self.env.cwd,
                local_stdin.as_deref(),
            )? {
                result
            } else if self.env.option_enabled("autocd") && args.is_empty() && self.resolve_path(&name).is_dir() {
                self.shell_builtin("cd", &[name.clone()], None)?
                    .unwrap_or_else(ExecutionResult::success)
            } else {
                let program = self.command_hash.get(&name).cloned().unwrap_or_else(|| name.clone());
                match self.host.execute_external(
                    &program,
                    args,
                    &self.env.cwd,
                    &self.env.exported,
                    local_stdin.as_deref(),
                ) {
                    Ok(result) => result,
                    Err(error) => ExecutionResult::from_parts(
                        String::new(),
                        format!("{name}: {error}\n"),
                        127,
                    ),
                }
            }
        } else if let Some(result) = self.host.execute_builtin(
            &name,
            args,
            &self.env.cwd,
            local_stdin.as_deref(),
        )? {
            result
        } else {
            let program = self.command_hash.get(&name).cloned().unwrap_or_else(|| name.clone());
            match self.host.execute_external(
                &program,
                args,
                &self.env.cwd,
                &self.env.exported,
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
            result.stderr = format!("{trace}{}", result.stderr);
        }
        self.apply_output_redirects(command, &mut result)?;
        self.finish_simple_result(checkpoint, true, result)
    }

    fn shell_builtin(
        &mut self,
        name: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>> {
        let result = match name {
            "cd" => {
                let raw = args.first().map(String::as_str).unwrap_or("~");
                let target = if raw == "-" {
                    self.env.oldpwd.clone().unwrap_or_else(|| self.env.cwd.clone())
                } else {
                    self.resolve_path(raw)
                };
                match fs::canonicalize(&target) {
                    Ok(path) if path.is_dir() => {
                        let previous = self.env.cwd.clone();
                        self.env.cwd = path.clone();
                        self.env.oldpwd = Some(previous.clone());
                        self.env.set("OLDPWD", previous.to_string_lossy().into_owned());
                        self.env.set("PWD", path.to_string_lossy().into_owned());
                        let stdout = if raw == "-" { format!("{}\n", path.display()) } else { String::new() };
                        ExecutionResult::from_parts(stdout, String::new(), 0)
                    }
                    Ok(_) => ExecutionResult::from_parts(String::new(), format!("cd: {raw}: no es un directorio\n"), 1),
                    Err(error) => ExecutionResult::from_parts(String::new(), format!("cd: {raw}: {error}\n"), 1),
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
                if args.is_empty() || args.iter().any(|arg| arg == "-p") {
                    let mut values = self.env.exported.iter().collect::<Vec<_>>();
                    values.sort_by_key(|(name, _)| *name);
                    let stdout = values.into_iter()
                        .map(|(name, value)| format!("declare -x {}={}\n", name, shell_quote(value)))
                        .collect();
                    ExecutionResult::from_parts(stdout, String::new(), 0)
                } else {
                    let mut status = 0;
                    let mut stderr = String::new();
                    for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
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
                            stderr.push_str(&format!("unset: {item}: no es nameref o es de solo lectura\n"));
                        }
                        continue;
                    }
                    if !variables_only && self.env.functions.remove(item).is_some() {
                        if functions_only { continue; }
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
            "exec" => {
                if args.is_empty() {
                    ExecutionResult::success()
                } else {
                    let mut command_index = 0usize;
                    while command_index < args.len() && args[command_index].starts_with('-') {
                        if args[command_index] == "--" {
                            command_index += 1;
                            break;
                        }
                        // -a/-c/-l are accepted syntactically; process replacement
                        // semantics are represented by terminating this shell after
                        // the command completes on Windows.
                        if args[command_index] == "-a" {
                            command_index += 2;
                        } else {
                            command_index += 1;
                        }
                    }
                    let Some(command) = args.get(command_index) else {
                        return Ok(Some(ExecutionResult::success()));
                    };
                    let mut execution = self.execute_command_direct(
                        command,
                        &args[command_index + 1..],
                        stdin,
                    )?;
                    if execution.status == 0 || !self.env.option_enabled("execfail") {
                        execution.exit_requested = true;
                    }
                    execution
                }
            }
            "enable" => self.builtin_enable(args),
            "exit" | "logout" => {
                let status = args.first()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(self.env.last_status);
                let mut result = ExecutionResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    status,
                    exit_requested: true,
                    flow: FlowSignal::None,
                };
                if let Some(action) = self.env.traps.get("EXIT").cloned().or_else(|| self.env.traps.get("0").cloned()) {
                    let mut trap_result = self.execute_text(&action)?;
                    trap_result.exit_requested = true;
                    trap_result.status = status;
                    result.append(trap_result);
                    result.exit_requested = true;
                    result.status = status;
                }
                result
            }
            "source" | "." => {
                let mut index = 0usize;
                let mut search_path: Option<&str> = None;
                if args.get(index).map(String::as_str) == Some("-p") {
                    index += 1;
                    let Some(path) = args.get(index) else {
                        return Ok(Some(ExecutionResult::from_parts(
                            String::new(),
                            format!("{name}: -p requiere PATH\n"),
                            2,
                        )));
                    };
                    search_path = Some(path);
                    index += 1;
                }
                if args.get(index).map(String::as_str) == Some("--") {
                    index += 1;
                }
                let Some(path) = args.get(index) else {
                    return Ok(Some(ExecutionResult::from_parts(
                        String::new(),
                        format!("{name}: falta archivo\n"),
                        2,
                    )));
                };
                let resolved = self.resolve_source_path(path, search_path);
                let source = fs::read_to_string(&resolved)?;
                let saved_positional = self.env.positional.clone();
                let saved_name = self.env.script_name.clone();
                let source_args = &args[index + 1..];
                if !source_args.is_empty() {
                    self.env.positional = source_args.to_vec();
                }
                self.source_depth += 1;
                self.source_stack.push(resolved.to_string_lossy().into_owned());
                let execution = self.execute_text(&source);
                self.source_stack.pop();
                self.source_depth = self.source_depth.saturating_sub(1);
                self.env.positional = saved_positional;
                self.env.script_name = saved_name;
                let mut result = execution?;
                if result.flow == FlowSignal::Return { result.flow = FlowSignal::None; }
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
                let mut stdout = String::new();
                if args.is_empty() || args.iter().any(|arg| arg == "-p") {
                    let mut names = self.env.readonly.iter().cloned().collect::<Vec<_>>();
                    names.sort();
                    for name in names {
                        stdout.push_str(&format!("declare -r {}={}\n", name, shell_quote(&self.env.get(&name))));
                    }
                    ExecutionResult::from_parts(stdout, String::new(), 0)
                } else {
                    for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
                        if let Some((name, value)) = arg.split_once('=') {
                            let value = self.expand_scalar(value)?;
                            self.env.set(name.to_owned(), value);
                            self.env.set_readonly(name);
                        } else {
                            self.env.set_readonly(arg);
                        }
                    }
                    ExecutionResult::success()
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
                    ExecutionResult::from_parts(String::new(), format!("shift: {count}: cantidad fuera de rango\n"), 1)
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
            "jobs" => self.builtin_jobs(args)?,
            "wait" => self.builtin_wait(args)?,
            "fg" => self.builtin_fg(args)?,
            "bg" => self.builtin_bg(args)?,
            "disown" => self.builtin_disown(args)?,
            "command" => {
                if args.first().map(String::as_str) == Some("-v") || args.first().map(String::as_str) == Some("-V") {
                    let mut stdout = String::new();
                    let mut status = 0;
                    for item in &args[1..] {
                        if self.env.aliases.contains_key(item) {
                            stdout.push_str(&format!("{item}\n"));
                        } else if self.env.functions.contains_key(item) {
                            stdout.push_str(&format!("{item}\n"));
                        } else if self.shell_builtin_name(item) {
                            stdout.push_str(&format!("{item}\n"));
                        } else {
                            match self.host.execute_builtin("which", &[item.clone()], &self.env.cwd, None)? {
                                Some(found) if found.status == 0 => stdout.push_str(&found.stdout),
                                _ => status = 1,
                            }
                        }
                    }
                    ExecutionResult::from_parts(stdout, String::new(), status)
                } else if let Some(command) = args.first() {
                    self.execute_command_direct(command, &args[1..], stdin)?
                } else {
                    ExecutionResult::success()
                }
            }
            "builtin" => {
                let Some(command) = args.first() else { return Ok(Some(ExecutionResult::success())); };
                self.shell_builtin(command, &args[1..], stdin)?
                    .unwrap_or_else(|| ExecutionResult::from_parts(String::new(), format!("builtin: {command}: no es builtin\n"), 1))
            }
            "type" => self.builtin_type(args)?,
            "hash" => self.builtin_hash(args)?,
            "getopts" => self.builtin_getopts(args)?,
            "dirs" => {
                let mut stdout = self.env.cwd.to_string_lossy().into_owned();
                for path in self.env.dir_stack.iter().rev() {
                    stdout.push(' ');
                    stdout.push_str(&path.to_string_lossy());
                }
                stdout.push('\n');
                ExecutionResult::from_parts(stdout, String::new(), 0)
            }
            "pushd" => {
                let target = args.first().map(|arg| self.resolve_path(arg))
                    .or_else(|| self.env.dir_stack.pop());
                if let Some(target) = target {
                    if target.is_dir() {
                        let current = self.env.cwd.clone();
                        self.env.dir_stack.push(current);
                        self.env.cwd = fs::canonicalize(target)?;
                        ExecutionResult::from_parts(format!("{}\n", self.env.cwd.display()), String::new(), 0)
                    } else {
                        ExecutionResult::from_parts(String::new(), "pushd: directorio inválido\n".to_owned(), 1)
                    }
                } else {
                    ExecutionResult::from_parts(String::new(), "pushd: pila vacía\n".to_owned(), 1)
                }
            }
            "popd" => {
                if let Some(target) = self.env.dir_stack.pop() {
                    self.env.cwd = target;
                    ExecutionResult::from_parts(format!("{}\n", self.env.cwd.display()), String::new(), 0)
                } else {
                    ExecutionResult::from_parts(String::new(), "popd: pila vacía\n".to_owned(), 1)
                }
            }
            "umask" => {
                if let Some(value) = args.first() {
                    self.env.set("__UMASK", value.clone());
                    ExecutionResult::success()
                } else {
                    ExecutionResult::from_parts(
                        format!("{}\n", self.env.vars.get("__UMASK").cloned().unwrap_or_else(|| "0022".to_owned())),
                        String::new(),
                        0,
                    )
                }
            }
            "ulimit" => self.builtin_ulimit(args),
            "times" => self.builtin_times()?,
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
        matches!(
            name,
            "cd" | "pwd" | "echo" | "printf" | "export" | "unset" | "alias" | "unalias"
                | "exit" | "logout" | "exec" | "source" | "." | "read" | "local" | "declare" | "typeset"
                | "readonly" | "break" | "continue" | "return" | "shift" | "set" | "shopt"
                | "trap" | "eval" | "let" | "test" | "[" | "mapfile" | "readarray" | "jobs"
                | "wait" | "fg" | "bg" | "disown" | "command" | "builtin" | "type" | "hash" | "getopts"
                | "dirs" | "pushd" | "popd" | "umask" | "ulimit" | "times" | "caller"
                | "enable" | ":" | "true" | "false"
        )
    }

    fn resolve_jobspec(&self, spec: Option<&str>) -> Result<Option<(usize, JobInfo)>> {
        let jobs = self.host.jobs()?;
        if jobs.is_empty() {
            return Ok(None);
        }

        let selected = match spec {
            None | Some("%") | Some("%%") | Some("%+") => jobs.len().checked_sub(1),
            Some("%-") => jobs.len().checked_sub(2),
            Some(value) if value.starts_with("%?") => {
                let needle = &value[2..];
                jobs.iter().position(|job| job.command.contains(needle))
            }
            Some(value) if value.starts_with('%') => {
                let tail = &value[1..];
                if let Ok(number) = tail.parse::<usize>() {
                    number.checked_sub(1).filter(|index| *index < jobs.len())
                } else {
                    jobs.iter().position(|job| job.command.starts_with(tail))
                }
            }
            Some(value) => value.parse::<u32>().ok()
                .and_then(|pid| jobs.iter().position(|job| job.pid == pid)),
        };

        Ok(selected.map(|index| (index + 1, jobs[index].clone())))
    }

    fn builtin_jobs(&self, args: &[String]) -> Result<ExecutionResult> {
        let jobs = self.host.jobs()?;
        let pids_only = args.iter().any(|arg| arg == "-p");
        let running_only = args.iter().any(|arg| arg == "-r");
        let stopped_only = args.iter().any(|arg| arg == "-s");
        let requested: Vec<&str> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(String::as_str)
            .collect();

        let mut stdout = String::new();
        for (index, job) in jobs.iter().enumerate() {
            if running_only && !job.running { continue; }
            // Native Windows jobs currently have Running/Done states. There is no
            // synthetic "Stopped" state: -s therefore reports none rather than lying.
            if stopped_only { continue; }
            if !requested.is_empty() {
                let number = format!("%{}", index + 1);
                if !requested.iter().any(|spec| {
                    *spec == number
                        || spec.parse::<u32>().ok() == Some(job.pid)
                        || spec.strip_prefix("%?").is_some_and(|needle| job.command.contains(needle))
                        || spec.strip_prefix('%').is_some_and(|prefix| job.command.starts_with(prefix))
                }) {
                    continue;
                }
            }
            if pids_only {
                stdout.push_str(&format!("{}\n", job.pid));
            } else {
                stdout.push_str(&format!(
                    "[{}] {} {} {}\n",
                    index + 1,
                    if job.running { "Running" } else { "Done" },
                    job.pid,
                    job.command
                ));
            }
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
        if !job.running {
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

    fn sync_call_stack_vars(&mut self) {
        let names = self.call_stack.iter().rev()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let sources = self.call_stack.iter().rev()
            .map(|(_, source)| source.clone())
            .collect::<Vec<_>>();
        let lines = vec!["0".to_owned(); self.call_stack.len()];
        self.env.set_array("FUNCNAME", names);
        self.env.set_array("BASH_SOURCE", sources);
        self.env.set_array("BASH_LINENO", lines);
    }

    fn builtin_caller(&self, args: &[String]) -> ExecutionResult {
        let frame = args.first()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let Some(index) = self.call_stack.len().checked_sub(frame + 1) else {
            return ExecutionResult::from_parts(String::new(), String::new(), 1);
        };
        let (function, source) = &self.call_stack[index];
        ExecutionResult::from_parts(
            format!("0 {function} {source}\n"),
            String::new(),
            0,
        )
    }

    fn builtin_times(&self) -> Result<ExecutionResult> {
        let (user, system, child_user, child_system) = self.host.shell_times()?;
        fn format_cpu(value: Duration) -> String {
            let total_millis = value.as_millis();
            let minutes = total_millis / 60_000;
            let seconds = (total_millis % 60_000) / 1000;
            let millis = total_millis % 1000;
            format!("{minutes}m{seconds}.{millis:03}s")
        }
        Ok(ExecutionResult::from_parts(
            format!(
                "{} {}\n{} {}\n",
                format_cpu(user),
                format_cpu(system),
                format_cpu(child_user),
                format_cpu(child_system),
            ),
            String::new(),
            0,
        ))
    }

    fn builtin_ulimit(&self, args: &[String]) -> ExecutionResult {
        const RESOURCES: &[(&str, &str)] = &[
            ("-c", "core file size"),
            ("-d", "data seg size"),
            ("-e", "scheduling priority"),
            ("-f", "file size"),
            ("-i", "pending signals"),
            ("-l", "max locked memory"),
            ("-m", "max memory size"),
            ("-n", "open files"),
            ("-p", "pipe size"),
            ("-q", "POSIX message queues"),
            ("-r", "real-time priority"),
            ("-s", "stack size"),
            ("-t", "cpu time"),
            ("-u", "max user processes"),
            ("-v", "virtual memory"),
            ("-x", "file locks"),
        ];

        if args.iter().any(|arg| arg == "-a") {
            let mut stdout = String::new();
            for (flag, label) in RESOURCES {
                stdout.push_str(&format!("{label:<28} ({flag}) unlimited\n"));
            }
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        let mut selected = "-f";
        let mut value: Option<&str> = None;
        for arg in args {
            if arg == "-S" || arg == "-H" {
                continue;
            }
            if RESOURCES.iter().any(|(flag, _)| *flag == arg) {
                selected = arg;
            } else if !arg.starts_with('-') {
                value = Some(arg);
            } else {
                return ExecutionResult::from_parts(
                    String::new(),
                    format!("ulimit: opción no válida: {arg}\n"),
                    2,
                );
            }
        }

        if value.is_some() {
            return ExecutionResult::from_parts(
                String::new(),
                format!("ulimit: {selected}: Windows no expone un rlimit POSIX modificable para este recurso\n"),
                1,
            );
        }

        ExecutionResult::from_parts("unlimited\n".to_owned(), String::new(), 0)
    }

    fn builtin_enable(&mut self, args: &[String]) -> ExecutionResult {
        let disable = args.iter().any(|arg| arg == "-n");
        let print_all = args.is_empty() || args.iter().any(|arg| arg == "-a" || arg == "-p");
        let names: Vec<&str> = args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(String::as_str)
            .collect();

        if print_all && names.is_empty() {
            let known = [
                ":", ".", "alias", "bg", "break", "builtin", "caller", "cd",
                "command", "continue", "declare", "dirs", "disown", "echo", "enable",
                "eval", "exec", "exit", "export", "false", "fg", "getopts", "hash",
                "jobs", "let", "local", "logout", "mapfile", "popd", "printf", "pushd",
                "pwd", "read", "readarray", "readonly", "return", "set", "shift",
                "shopt", "source", "test", "times", "trap", "true", "type", "typeset",
                "ulimit", "umask", "unalias", "unset", "wait",
            ];
            let mut stdout = String::new();
            for name in known {
                stdout.push_str(&format!(
                    "enable {}{}\n",
                    if self.disabled_builtins.contains(name) { "-n " } else { "" },
                    name,
                ));
            }
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        let mut status = 0;
        let mut stderr = String::new();
        for name in names {
            if !self.shell_builtin_name(name) {
                status = 1;
                stderr.push_str(&format!("enable: {name}: no es builtin de shell\n"));
                continue;
            }
            if disable {
                self.disabled_builtins.insert(name.to_owned());
            } else {
                self.disabled_builtins.remove(name);
            }
        }
        ExecutionResult::from_parts(String::new(), stderr, status)
    }

    fn builtin_echo(&self, args: &[String]) -> ExecutionResult {
        let mut newline = true;
        let mut escapes = false;
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
                return Ok(ExecutionResult::from_parts(String::new(), "printf: -v requiere variable\n".to_owned(), 2));
            };
            assign_to = Some(name.clone());
            args = &args[2..];
        }

        let Some(format) = args.first() else {
            return Ok(ExecutionResult::from_parts(String::new(), "printf: falta formato\n".to_owned(), 2));
        };
        let values = &args[1..];
        let mut output = String::new();
        let mut value_index = 0usize;
        let chars: Vec<char> = format.chars().collect();
        let mut i = 0usize;

        while i < chars.len() {
            if chars[i] == '\\' {
                let rest: String = chars[i..].iter().collect();
                let (decoded, stop) = decode_backslash_escapes(&rest, true);
                output.push_str(&decoded);
                if stop { break; }
                break;
            }
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
            let mut width = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '-' | '+' | '0' | ' ' | '.')) {
                width.push(chars[i]);
                i += 1;
            }
            let spec = chars.get(i).copied().unwrap_or(' ');
            if i < chars.len() { i += 1; }
            let value = values.get(value_index).cloned().unwrap_or_default();
            value_index += 1;

            let rendered = match spec {
                's' => value,
                'q' => shell_quote(&value),
                'b' => decode_backslash_escapes(&value, true).0,
                'd' | 'i' => value.parse::<i64>().unwrap_or(0).to_string(),
                'u' => value.parse::<u64>().unwrap_or(0).to_string(),
                'x' => format!("{:x}", value.parse::<i64>().unwrap_or(0)),
                'X' => format!("{:X}", value.parse::<i64>().unwrap_or(0)),
                'o' => format!("{:o}", value.parse::<i64>().unwrap_or(0)),
                'c' => value.chars().next().map(|c| c.to_string()).unwrap_or_default(),
                _ => format!("%{width}{spec}"),
            };
            output.push_str(&rendered);
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
        let mut delimiter: Option<char> = None;
        let mut timeout: Option<Duration> = None;
        let mut input_fd = 0i32;
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
                    // The native terminal already supplies line editing. -E selects
                    // the shell's standard completion set, while -e permits current
                    // Readline-style bindings; both use the same native editor here.
                }
                "-i" => {
                    index += 1;
                    initial = args.get(index).cloned().unwrap_or_default();
                }
                "-d" => {
                    index += 1;
                    delimiter = Some(
                        args.get(index)
                            .and_then(|value| value.chars().next())
                            .unwrap_or('\0'),
                    );
                }
                "-t" => {
                    index += 1;
                    let Some(value) = args.get(index).and_then(|value| value.parse::<f64>().ok()) else {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "read: -t requiere un número de segundos válido\n".to_owned(),
                            2,
                        ));
                    };
                    if value.is_sign_negative() {
                        return Ok(ExecutionResult::from_parts(
                            String::new(),
                            "read: timeout negativo\n".to_owned(),
                            2,
                        ));
                    }
                    timeout = Some(Duration::from_secs_f64(value));
                }
                "-u" => {
                    index += 1;
                    input_fd = args.get(index).and_then(|value| value.parse::<i32>().ok()).unwrap_or(-1);
                }
                "-n" => {
                    index += 1;
                    max_chars = args.get(index).and_then(|value| value.parse::<usize>().ok());
                    exact_chars = false;
                }
                "-N" => {
                    index += 1;
                    max_chars = args.get(index).and_then(|value| value.parse::<usize>().ok());
                    exact_chars = true;
                }
                "-a" => {
                    index += 1;
                    array_name = args.get(index).cloned();
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

        if input_fd != 0 {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("read: descriptor {input_fd}: no disponible en esta sesión\n"),
                1,
            ));
        }

        let source = if let Some(bytes) = stdin {
            let text = String::from_utf8_lossy(bytes);
            let mut value = if exact_chars {
                text.chars().take(max_chars.unwrap_or(usize::MAX)).collect::<String>()
            } else if let Some(delim) = delimiter {
                text.split(delim).next().unwrap_or("").to_owned()
            } else {
                text.lines().next().unwrap_or("").to_owned()
            };
            if !initial.is_empty() {
                value.insert_str(0, &initial);
            }
            Some(value)
        } else {
            self.host.read_line_with_options(
                &prompt,
                silent,
                &initial,
                timeout,
                delimiter,
                max_chars,
                exact_chars,
            )?
        };

        let Some(mut value) = source else {
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        };

        if let Some(max) = max_chars {
            value = value.chars().take(max).collect();
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
        let mut remove_nameref = false;
        let mut global = false;
        let mut names = Vec::new();

        for arg in args {
            if (arg.starts_with('-') || arg.starts_with('+')) && arg.len() > 1 {
                let enable = arg.starts_with('-');
                for flag in arg[1..].chars() {
                    match flag {
                        'a' if enable => indexed = true,
                        'A' if enable => associative = true,
                        'r' if enable => readonly = true,
                        'x' if enable => export = true,
                        'p' if enable => print = true,
                        'i' if enable => integer = true,
                        'n' if enable => nameref = true,
                        'n' => remove_nameref = true,
                        'g' if enable => global = true,
                        _ => {}
                    }
                }
            } else {
                names.push(arg.clone());
            }
        }

        if print || names.is_empty() {
            let mut stdout = String::new();
            let mut all: Vec<_> = self.env.vars.keys().cloned().collect();
            all.sort();
            for name in all {
                stdout.push_str(&format!("declare -- {}={}\n", name, shell_quote(&self.env.get(&name))));
            }
            let mut refs: Vec<_> = self.env.namerefs.iter().collect();
            refs.sort_by_key(|(name, _)| *name);
            for (name, target) in refs {
                stdout.push_str(&format!("declare -n {name}={}\n", shell_quote(target)));
            }
            for (name, values) in &self.env.arrays {
                stdout.push_str(&format!("declare -a {name}=("));
                for (index, value) in values.iter().enumerate() {
                    if !value.is_empty() { stdout.push_str(&format!("[{index}]={} ", shell_quote(value))); }
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

        let effective_local = local || (!global && !self.env.local_scopes.is_empty());

        for item in names {
            let (name, mut value) = item.split_once('=')
                .map(|(n,v)| (n.to_owned(), Some(v.to_owned())))
                .unwrap_or((item.clone(), None));

            if remove_nameref {
                if let Some(target) = self.env.namerefs.get(&name).cloned() {
                    let _ = self.env.unset_nameref(&name);
                    if effective_local { self.env.set_local(name.clone(), target); }
                    else { self.env.set(name.clone(), target); }
                }
                continue;
            }

            if nameref {
                let target = if let Some(raw) = value.take() {
                    self.expand_scalar(&raw)?
                } else {
                    String::new()
                };
                if effective_local {
                    self.env.set_local_nameref(name.clone(), target);
                } else {
                    self.env.set_nameref(name.clone(), target);
                }
                if readonly { self.env.set_readonly(&name); }
                continue;
            }

            if associative {
                if effective_local { self.env.declare_local_assoc(name.clone()); }
                else { self.env.declare_assoc(name.clone()); }
            } else if indexed {
                if effective_local { self.env.set_local_array(name.clone(), Vec::new()); }
                else { self.env.set_array(name.clone(), Vec::new()); }
            }

            if let Some(raw_value) = value.take() {
                if raw_value.starts_with('(') && raw_value.ends_with(')') && (indexed || associative) {
                    let body = &raw_value[1..raw_value.len() - 1];
                    let items = split_shell_words_relaxed(body)?;
                    if associative {
                        for item in items {
                            if let Some((key, value)) = parse_array_entry(&item) {
                                let expanded = self.expand_scalar(&value)?;
                                self.env.assoc_arrays.entry(name.clone()).or_default().insert(key, expanded);
                            }
                        }
                    } else {
                        let mut values = Vec::new();
                        for item in items {
                            if let Some((key, value)) = parse_array_entry(&item) {
                                if let Ok(index) = key.parse::<usize>() {
                                    if values.len() <= index { values.resize(index + 1, String::new()); }
                                    values[index] = self.expand_scalar(&value)?;
                                }
                            } else {
                                values.push(self.expand_scalar(&item)?);
                            }
                        }
                        if effective_local { self.env.set_local_array(name.clone(), values); }
                        else { self.env.set_array(name.clone(), values); }
                    }
                } else {
                    let expanded = if integer {
                        self.evaluate_arithmetic_command(&raw_value)?.to_string()
                    } else {
                        self.expand_scalar(&raw_value)?
                    };
                    if effective_local { self.env.set_local(name.clone(), expanded); }
                    else { self.env.set(name.clone(), expanded); }
                }
            } else if effective_local && !indexed && !associative {
                let current = self.env.get(&name);
                self.env.set_local(name.clone(), current);
            }

            if readonly { self.env.set_readonly(&name); }
            if export { self.env.mark_exported(&name); }
        }

        Ok(ExecutionResult::success())
    }

    fn builtin_set(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.is_empty() {
            let mut rows = self.env.vars.iter().collect::<Vec<_>>();
            rows.sort_by_key(|(name, _)| *name);
            let stdout = rows.into_iter().map(|(name,value)| format!("{name}={}\n", shell_quote(value))).collect();
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
                    set_shell_option(&mut self.env, option, enable);
                    index += 2;
                    continue;
                }
                let mut options = self.env.shell_options.iter().cloned().collect::<Vec<_>>();
                options.sort();
                let stdout = options.into_iter().map(|option| format!("set -o {option}\n")).collect();
                return Ok(ExecutionResult::from_parts(stdout, String::new(), 0));
            }
            if arg.starts_with('-') || arg.starts_with('+') {
                let enable = arg.starts_with('-');
                for flag in arg[1..].chars() {
                    match flag {
                        'e' => set_shell_option(&mut self.env, "errexit", enable),
                        'u' => set_shell_option(&mut self.env, "nounset", enable),
                        'x' => set_shell_option(&mut self.env, "xtrace", enable),
                        'f' => set_shell_option(&mut self.env, "noglob", enable),
                        'n' => set_shell_option(&mut self.env, "noexec", enable),
                        'C' => set_shell_option(&mut self.env, "noclobber", enable),
                        'a' => set_shell_option(&mut self.env, "allexport", enable),
                        'm' => set_shell_option(&mut self.env, "monitor", enable),
                        'b' => set_shell_option(&mut self.env, "notify", enable),
                        'h' => set_shell_option(&mut self.env, "hashall", enable),
                        'B' => set_shell_option(&mut self.env, "braceexpand", enable),
                        'H' => set_shell_option(&mut self.env, "histexpand", enable),
                        'P' => set_shell_option(&mut self.env, "physical", enable),
                        'v' => set_shell_option(&mut self.env, "verbose", enable),
                        _ => {}
                    }
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
        let print = args.is_empty() || args.iter().any(|arg| arg == "-p");
        let names: Vec<_> = args.iter().filter(|arg| !arg.starts_with('-')).cloned().collect();

        if print && names.is_empty() {
            let known = [
                "array_expand_once", "autocd", "cdspell", "checkwinsize", "dotglob",
                "execfail", "expand_aliases", "extglob", "failglob", "globskipdots",
                "globstar", "inherit_errexit", "lastpipe", "nocaseglob", "nocasematch", "nullglob",
                "patsub_replacement", "progcomp", "sourcepath",
            ];
            let mut stdout = String::new();
            for option in known {
                stdout.push_str(&format!(
                    "shopt {} {}\n",
                    if self.env.shopt_options.contains(option) { "-s" } else { "-u" },
                    option
                ));
            }
            return ExecutionResult::from_parts(stdout, String::new(), 0);
        }

        for name in names {
            if enable { self.env.shopt_options.insert(name); }
            else if disable { self.env.shopt_options.remove(&name); }
        }
        ExecutionResult::success()
    }

    fn builtin_trap(&mut self, args: &[String]) -> ExecutionResult {
        if args.is_empty() || args.first().map(String::as_str) == Some("-p") {
            let mut traps = self.env.traps.iter().collect::<Vec<_>>();
            traps.sort_by_key(|(signal, _)| *signal);
            let stdout = traps.into_iter()
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
        let mut trim_delimiter = false;
        let mut skip = 0usize;
        let mut count: Option<usize> = None;
        let mut origin: Option<usize> = None;
        let mut delimiter = '\n';
        let mut input_fd = 0i32;
        let mut callback: Option<String> = None;
        let mut quantum = 5000usize;
        let mut name = "MAPFILE".to_owned();
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "-t" => trim_delimiter = true,
                "-s" => {
                    index += 1;
                    skip = args.get(index).and_then(|value| value.parse().ok()).unwrap_or(0);
                }
                "-n" => {
                    index += 1;
                    count = args.get(index).and_then(|value| value.parse().ok());
                }
                "-O" => {
                    index += 1;
                    origin = args.get(index).and_then(|value| value.parse().ok());
                }
                "-d" => {
                    index += 1;
                    delimiter = args.get(index)
                        .and_then(|value| value.chars().next())
                        .unwrap_or('\0');
                }
                "-u" => {
                    index += 1;
                    input_fd = args.get(index).and_then(|value| value.parse().ok()).unwrap_or(-1);
                }
                "-C" => {
                    index += 1;
                    callback = args.get(index).cloned();
                }
                "-c" => {
                    index += 1;
                    quantum = args.get(index)
                        .and_then(|value| value.parse::<usize>().ok())
                        .filter(|value| *value > 0)
                        .unwrap_or(5000);
                }
                "--" => {
                    if let Some(value) = args.get(index + 1) {
                        name = value.clone();
                    }
                    break;
                }
                value if value.starts_with('-') => {
                    return Ok(ExecutionResult::from_parts(
                        String::new(),
                        format!("mapfile: opción no válida: {value}\n"),
                        2,
                    ));
                }
                value => name = value.to_owned(),
            }
            index += 1;
        }

        if input_fd != 0 {
            return Ok(ExecutionResult::from_parts(
                String::new(),
                format!("mapfile: descriptor {input_fd}: no disponible en esta sesión\n"),
                1,
            ));
        }

        let text = if let Some(bytes) = stdin {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            let mut all = String::new();
            while let Some(line) = self.host.read_line("", false)? {
                all.push_str(&line);
                all.push('\n');
            }
            all
        };

        let mut rows = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            if ch == delimiter {
                if trim_delimiter {
                    current.pop();
                }
                rows.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            rows.push(current);
        }

        let mut rows: Vec<String> = rows.into_iter().skip(skip).collect();
        if let Some(limit) = count {
            if limit > 0 {
                rows.truncate(limit);
            }
        }

        let start_index = origin.unwrap_or(0);
        let mut target = if origin.is_some() {
            self.env.array_values(&name)
        } else {
            Vec::new()
        };

        for (offset, row) in rows.into_iter().enumerate() {
            let array_index = start_index + offset;
            if target.len() <= array_index {
                target.resize(array_index + 1, String::new());
            }

            if let Some(action) = callback.as_ref() {
                if offset % quantum == 0 {
                    let command = format!(
                        "{} {} {}",
                        action,
                        array_index,
                        shell_quote(&row),
                    );
                    let callback_result = self.execute_text(&command)?;
                    if callback_result.status != 0 {
                        return Ok(callback_result);
                    }
                }
            }

            target[array_index] = row;
        }

        self.env.set_array(name, target);
        Ok(ExecutionResult::success())
    }

    fn builtin_type(&mut self, args: &[String]) -> Result<ExecutionResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut status = 0;
        for name in args {
            if let Some(value) = self.env.aliases.get(name) {
                stdout.push_str(&format!("{name} es un alias de {}\n", shell_quote(value)));
            } else if self.env.functions.contains_key(name) {
                stdout.push_str(&format!("{name} es una función\n"));
            } else if self.shell_builtin_name(name) {
                stdout.push_str(&format!("{name} es un builtin de shell\n"));
            } else {
                match self.host.execute_builtin("which", &[name.clone()], &self.env.cwd, None)? {
                    Some(result) if result.status == 0 => stdout.push_str(&result.stdout),
                    _ => {
                        status = 1;
                        stderr.push_str(&format!("type: {name}: no encontrado\n"));
                    }
                }
            }
        }
        Ok(ExecutionResult::from_parts(stdout, stderr, status))
    }

    fn builtin_getopts(&mut self, args: &[String]) -> Result<ExecutionResult> {
        if args.len() < 2 {
            return Ok(ExecutionResult::from_parts(String::new(), "getopts: uso: getopts optstring name [args]\n".to_owned(), 2));
        }
        let optstring = &args[0];
        let varname = &args[1];
        let source: Vec<String> = if args.len() > 2 { args[2..].to_vec() } else { self.env.positional.clone() };
        let optind = self.env.get("OPTIND").parse::<usize>().unwrap_or(1).max(1);
        let Some(item) = source.get(optind - 1) else {
            self.env.set(varname.clone(), "?");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        };
        if !item.starts_with('-') || item == "-" || item == "--" {
            self.env.set(varname.clone(), "?");
            return Ok(ExecutionResult::from_parts(String::new(), String::new(), 1));
        }
        let option = item.chars().nth(1).unwrap_or('?');
        let chars: Vec<char> = optstring.chars().collect();
        let requires_arg = if let Some(pos) = chars.iter().position(|ch| *ch == option) {
            chars.get(pos + 1) == Some(&':')
        } else {
            self.env.set(varname.clone(), "?");
            self.env.set("OPTARG", option.to_string());
            self.env.set("OPTIND", (optind + 1).to_string());
            return Ok(ExecutionResult::success());
        };
        self.env.set(varname.clone(), option.to_string());
        if requires_arg {
            if item.len() > 2 {
                self.env.set("OPTARG", item[2..].to_owned());
                self.env.set("OPTIND", (optind + 1).to_string());
            } else if let Some(value) = source.get(optind) {
                self.env.set("OPTARG", value.clone());
                self.env.set("OPTIND", (optind + 2).to_string());
            } else {
                self.env.set(varname.clone(), if optstring.starts_with(':') { ":" } else { "?" });
                self.env.set("OPTARG", option.to_string());
                self.env.set("OPTIND", (optind + 1).to_string());
            }
        } else {
            self.env.set("OPTARG", "");
            self.env.set("OPTIND", (optind + 1).to_string());
        }
        Ok(ExecutionResult::success())
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
        let program = self.command_hash.get(name).cloned().unwrap_or_else(|| name.to_owned());
        match self.host.execute_external(&program, args, &self.env.cwd, &self.env.exported, stdin) {
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
                    "-b" | "-c" | "-p" | "-S" => false,
                    "-t" => value.parse::<i32>().ok().is_some_and(|fd| (0..=2).contains(&fd)),
                    "-o" => self.env.option_enabled(&value),
                    _ => false,
                })
            }
            [left, op, right] => {
                let left = self.expand_scalar(left)?;
                let right = self.expand_scalar(right)?;
                let nocase = self.env.option_enabled("nocasematch");
                Ok(match op.as_str() {
                    "=" | "==" => shell_pattern_matches(
                        &right,
                        &left,
                        self.env.option_enabled("extglob"),
                        nocase,
                    ),
                    "!=" => !shell_pattern_matches(
                        &right,
                        &left,
                        self.env.option_enabled("extglob"),
                        nocase,
                    )
                    "=~" => {
                        match regex::RegexBuilder::new(&right)
                            .case_insensitive(nocase)
                            .build()
                        {
                            Ok(pattern) => {
                                if let Some(captures) = pattern.captures(&left) {
                                    let values = (0..captures.len())
                                        .map(|index| captures.get(index)
                                            .map(|value| value.as_str().to_owned())
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
                    let mut options = OpenOptions::new();
                    options.create(true).write(true);
                    if redirect.kind == RedirectKind::Append {
                        options.append(true);
                    } else {
                        options.truncate(true);
                    }
                    let mut file = options.open(path)?;
                    let data = if redirect.fd == 2 {
                        std::mem::take(&mut result.stderr)
                    } else {
                        std::mem::take(&mut result.stdout)
                    };
                    file.write_all(data.as_bytes())?;
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
                    let mut options = OpenOptions::new();
                    options.create(true).write(true);
                    if redirect.kind == RedirectKind::BothAppend {
                        options.append(true);
                    } else {
                        options.truncate(true);
                    }
                    let mut file = options.open(path)?;
                    let stdout = std::mem::take(&mut result.stdout);
                    let stderr = std::mem::take(&mut result.stderr);
                    file.write_all(stdout.as_bytes())?;
                    file.write_all(stderr.as_bytes())?;
                }
                RedirectKind::Dup => {
                    let target = self.expand_scalar(&redirect.target)?;
                    if target == "-" {
                        if redirect.fd == 2 { result.stderr.clear(); }
                        else if redirect.fd == 1 { result.stdout.clear(); }
                    } else if redirect.fd == 2 && target == "1" {
                        result.stdout.push_str(&result.stderr);
                        result.stderr.clear();
                    } else if redirect.fd == 1 && target == "2" {
                        result.stderr.push_str(&result.stdout);
                        result.stdout.clear();
                    }
                }
                RedirectKind::Read
                | RedirectKind::ReadWrite
                | RedirectKind::HereString => {}
            }
        }
        Ok(())
    }

    fn expansion_checkpoint(&self) -> ExpansionCheckpoint {
        ExpansionCheckpoint {
            stdout_len: self.pending_expansion_stdout.len(),
            stderr_len: self.pending_expansion_stderr.len(),
            statuses_len: self.pending_substitution_statuses.len(),
            exits_len: self.pending_expansion_exits.len(),
            process_sub_len: self.pending_process_substitutions.len(),
        }
    }

    fn finish_simple_result(
        &mut self,
        checkpoint: ExpansionCheckpoint,
        command_present: bool,
        mut result: ExecutionResult,
    ) -> Result<ExecutionResult> {
        let expansion_stdout = self.pending_expansion_stdout.split_off(checkpoint.stdout_len);
        let expansion_stderr = self.pending_expansion_stderr.split_off(checkpoint.stderr_len);

        let substitution_status = self.pending_substitution_statuses
            .get(checkpoint.statuses_len..)
            .and_then(|statuses| statuses.last())
            .copied();
        self.pending_substitution_statuses.truncate(checkpoint.statuses_len);

        let requested_exit = self.pending_expansion_exits
            .get(checkpoint.exits_len..)
            .and_then(|statuses| statuses.last())
            .copied();
        self.pending_expansion_exits.truncate(checkpoint.exits_len);

        if !command_present {
            if let Some(status) = substitution_status {
                result.status = status;
            }
        }
        if let Some(status) = requested_exit {
            result.status = status;
            result.exit_requested = true;
        }

        if !expansion_stdout.is_empty() {
            result.stdout = format!("{expansion_stdout}{}", result.stdout);
        }
        if !expansion_stderr.is_empty() {
            result.stderr = format!("{expansion_stderr}{}", result.stderr);
        }

        let process_output = self.finish_process_substitutions(checkpoint.process_sub_len)?;
        result.stdout.push_str(&process_output.stdout);
        result.stderr.push_str(&process_output.stderr);
        Ok(result)
    }

    fn next_process_substitution_path(&mut self) -> PathBuf {
        self.process_sub_counter = self.process_sub_counter.wrapping_add(1);
        std::env::temp_dir().join(format!(
            "shell-shock-psub-{}-{}.tmp",
            std::process::id(),
            self.process_sub_counter,
        ))
    }

    fn finish_process_substitutions(&mut self, start: usize) -> Result<ExecutionResult> {
        if start >= self.pending_process_substitutions.len() {
            return Ok(ExecutionResult::success());
        }

        let pending = self.pending_process_substitutions.split_off(start);
        let mut stdout = String::new();
        let mut stderr = String::new();

        for substitution in pending {
            stderr.push_str(&substitution.stderr);
            if let Some(command) = substitution.command {
                let input = fs::read(&substitution.path).unwrap_or_default();
                let node = parse(&command)?;
                let result = self.execute(&node, Some(&input))?;
                stdout.push_str(&result.stdout);
                stderr.push_str(&result.stderr);
            }
            let _ = fs::remove_file(&substitution.path);
        }

        Ok(ExecutionResult::from_parts(stdout, stderr, 0))
    }

    fn expand_words(&mut self, words: &[String]) -> Result<Vec<String>> {
        let mut result = Vec::new();

        for raw in words {
            for braced in brace_expand(raw) {
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
                let expanded = self.expand_scalar(&braced)?;

                if quoted {
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
                    let is_pattern = contains_glob_meta(&field)
                        || (self.env.option_enabled("extglob") && find_extglob(&field).is_some());
                    let paths = self.glob(&field)?;
                    if paths.is_empty() {
                        if self.env.option_enabled("failglob") && is_pattern {
                            bail!("no hay coincidencias: {field}");
                        }
                        if !self.env.option_enabled("nullglob") || !is_pattern {
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
            if !single
                && !double
                && matches!(chars[i], '<' | '>')
                && chars.get(i + 1) == Some(&'(')
            {
                let direction = chars[i];
                let end = matching(&chars, i + 1, '(', ')')
                    .ok_or_else(|| anyhow!("sustitución de proceso sin cerrar"))?;
                let source: String = chars[i + 2..end].iter().collect();
                let path = self.next_process_substitution_path();

                if direction == '<' {
                    let saved = self.env.clone();
                    let execution = self.execute_text(&source);
                    self.env = saved;
                    let execution = execution?;
                    fs::write(&path, execution.stdout.as_bytes())?;
                    self.pending_process_substitutions.push(ProcessSubstitution {
                        path: path.clone(),
                        command: None,
                        stderr: execution.stderr,
                    });
                } else {
                    fs::write(&path, b"")?;
                    self.pending_process_substitutions.push(ProcessSubstitution {
                        path: path.clone(),
                        command: Some(source),
                        stderr: String::new(),
                    });
                }

                out.push_str(&path.to_string_lossy());
                i = end + 1;
                continue;
            }

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
                    let saved_env = self.env.clone();
                    let saved_hash = self.command_hash.clone();
                    let saved_disabled = self.disabled_builtins.clone();
                    let result = self.execute_text(&source);
                    self.env = saved_env;
                    self.command_hash = saved_hash;
                    self.disabled_builtins = saved_disabled;
                    let result = result?;
                    self.pending_expansion_stderr.push_str(&result.stderr);
                    self.pending_substitution_statuses.push(result.status);
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
                        let saved_env = self.env.clone();
                        let saved_hash = self.command_hash.clone();
                        let saved_disabled = self.disabled_builtins.clone();
                        let result = self.execute_text(&source);
                        self.env = saved_env;
                        self.command_hash = saved_hash;
                        self.disabled_builtins = saved_disabled;
                        let result = result?;
                        self.pending_expansion_stderr.push_str(&result.stderr);
                        self.pending_substitution_statuses.push(result.status);
                        out.push_str(result.stdout.trim_end_matches(['\r', '\n']));
                        i = end + 1;
                    } else if chars.get(i + 1) == Some(&'{') {
                        let end = matching(&chars, i + 1, '{', '}')
                            .ok_or_else(|| anyhow!("expansión de parámetro sin cerrar"))?;
                        let expression: String = chars[i + 2..end].iter().collect();
                        if let Some(value) = self.expand_current_shell_substitution(&expression)? {
                            out.push_str(&value);
                        } else {
                            out.push_str(&self.expand_parameter(&expression)?);
                        }
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

    fn expand_current_shell_substitution(&mut self, expression: &str) -> Result<Option<String>> {
        let reply_mode = expression.starts_with('|');
        let capture_mode = expression.chars().next().is_some_and(char::is_whitespace);
        if !reply_mode && !capture_mode {
            return Ok(None);
        }

        let command_text = if reply_mode {
            &expression[1..]
        } else {
            expression
        };
        let trimmed = command_text.trim();
        let Some(command) = trimmed.strip_suffix(';') else {
            bail!("sustitución de comando en shell actual: falta ';' antes de '}}'");
        };
        let command = command.trim_end();

        self.env.push_local_scope();
        if reply_mode {
            let _ = self.env.localize_unset("REPLY");
        }

        let execution = self.execute_text(command);

        let reply = if reply_mode {
            self.env.get("REPLY")
        } else {
            String::new()
        };

        self.env.pop_local_scope();

        let mut execution = execution?;
        if execution.flow == FlowSignal::Return {
            execution.flow = FlowSignal::None;
        }

        self.pending_substitution_statuses.push(execution.status);
        self.pending_expansion_stderr.push_str(&execution.stderr);

        if execution.exit_requested {
            self.pending_expansion_exits.push(execution.status);
        }

        if reply_mode {
            self.pending_expansion_stdout.push_str(&execution.stdout);
            Ok(Some(reply))
        } else {
            Ok(Some(
                execution.stdout.trim_end_matches(['\r', '\n']).to_owned()
            ))
        }
    }

    fn special_value(&self, name: &str) -> String {
        match name {
            "RANDOM" => {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                (nanos % 32768).to_string()
            }
            "BASH_VERSION" => "5.3.0(1)-sst".to_owned(),
            "BASHPID" | "$" => std::process::id().to_string(),
            "PPID" => std::process::id().saturating_sub(1).to_string(),
            _ => self.env.get(name),
        }
    }

    fn expand_parameter(&mut self, expression: &str) -> Result<String> {
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

        for suffix in ["@Q", "@E", "@U", "@u", "@L", "@A", "@a"] {
            if let Some(name) = expression.strip_suffix(suffix) {
                let value = self.env.get(name);
                return Ok(match suffix {
                    "@Q" => shell_quote(&value),
                    "@E" => decode_backslash_escapes(&value, false).0,
                    "@U" => value.to_uppercase(),
                    "@u" => capitalize_first(&value),
                    "@L" => value.to_lowercase(),
                    "@A" => {
                        if self.env.arrays.contains_key(name) {
                            format!("declare -a {name}=({})",
                                self.env.array_values(name)
                                    .iter()
                                    .enumerate()
                                    .map(|(index, item)| format!("[{index}]={}", shell_quote(item)))
                                    .collect::<Vec<_>>()
                                    .join(" "))
                        } else if self.env.assoc_arrays.contains_key(name) {
                            let mut entries: Vec<_> = self.env.assoc_arrays[name].iter().collect();
                            entries.sort_by_key(|(key, _)| *key);
                            format!("declare -A {name}=({})",
                                entries.into_iter()
                                    .map(|(key, item)| format!("[{}]={}", shell_quote(key), shell_quote(item)))
                                    .collect::<Vec<_>>()
                                    .join(" "))
                        } else {
                            format!("declare -- {name}={}", shell_quote(&value))
                        }
                    }
                    "@a" => {
                        let mut attrs = String::new();
                        if self.env.arrays.contains_key(name) { attrs.push('a'); }
                        if self.env.assoc_arrays.contains_key(name) { attrs.push('A'); }
                        if self.env.is_nameref(name) { attrs.push('n'); }
                        if self.env.readonly.contains(name) { attrs.push('r'); }
                        if self.env.exported.contains_key(name) { attrs.push('x'); }
                        attrs
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

        let extglob = self.env.option_enabled("extglob");
        let patsub_replacement = self.env.option_enabled("patsub_replacement");

        if let Some(rest) = remainder.strip_prefix("//") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob(
                &self.env.get(name),
                pattern,
                replacement,
                true,
                extglob,
                patsub_replacement,
            ));
        }
        if let Some(rest) = remainder.strip_prefix("/#") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob_anchored(
                &self.env.get(name),
                pattern,
                replacement,
                true,
                extglob,
                patsub_replacement,
            ));
        }
        if let Some(rest) = remainder.strip_prefix("/%") {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob_anchored(
                &self.env.get(name),
                pattern,
                replacement,
                false,
                extglob,
                patsub_replacement,
            ));
        }
        if let Some(rest) = remainder.strip_prefix('/') {
            let (pattern, replacement) = rest.split_once('/').unwrap_or((rest, ""));
            return Ok(replace_glob(
                &self.env.get(name),
                pattern,
                replacement,
                false,
                extglob,
                patsub_replacement,
            ));
        }

        for operator in ["##", "#", "%%", "%"] {
            if let Some(pattern) = remainder.strip_prefix(operator) {
                return Ok(remove_glob_pattern(
                    &self.env.get(name),
                    pattern,
                    operator,
                    extglob,
                ));
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
            let home = self.env.get("USERPROFILE");
            if !home.is_empty() {
                return format!("{home}{}", &raw[1..]);
            }
        }
        if raw == "~+" {
            return self.env.cwd.to_string_lossy().into_owned();
        }
        if raw == "~-" {
            return self.env.oldpwd.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        }
        raw.to_owned()
    }

    fn glob(&self, value: &str) -> Result<Vec<String>> {
        let extglob_enabled = self.env.option_enabled("extglob");
        let has_pattern = contains_glob_meta(value)
            || (extglob_enabled && find_extglob(value).is_some());
        if !has_pattern {
            return Ok(Vec::new());
        }

        if extglob_enabled && find_extglob(value).is_some() {
            return self.glob_extglob(value);
        }

        let mut pattern = if Path::new(value).is_absolute() {
            value.to_owned()
        } else {
            self.env.cwd.join(value).to_string_lossy().into_owned()
        };
        if !self.env.option_enabled("globstar") {
            while pattern.contains("**") {
                pattern = pattern.replace("**", "*");
            }
        }
        let mut result = Vec::new();
        let options = glob::MatchOptions {
            case_sensitive: !self.env.option_enabled("nocaseglob"),
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };
        for entry in glob::glob_with(&pattern, options)? {
            let Ok(path) = entry else { continue };
            let globignore_active = !self.env.get("GLOBIGNORE").is_empty();
            if !self.env.option_enabled("dotglob") && !globignore_active {
                let hidden = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
                let explicit_hidden = Path::new(value).file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
                if hidden && !explicit_hidden { continue; }
            }
            if Path::new(value).is_absolute() {
                result.push(path.to_string_lossy().into_owned());
            } else if let Ok(relative) = path.strip_prefix(&self.env.cwd) {
                result.push(relative.to_string_lossy().into_owned());
            }
        }
        Ok(self.finalize_glob_results(result))
    }

    fn glob_extglob(&self, value: &str) -> Result<Vec<String>> {
        fn descendants(
            base: &Path,
            include_hidden: bool,
            output: &mut Vec<PathBuf>,
        ) {
            let Ok(entries) = fs::read_dir(base) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                if hidden && !include_hidden {
                    continue;
                }
                output.push(path.clone());
                if path.is_dir() {
                    descendants(&path, include_hidden, output);
                }
            }
        }

        let absolute = Path::new(value).is_absolute();
        let full = if absolute {
            PathBuf::from(value)
        } else {
            self.env.cwd.join(value)
        };

        let mut paths = vec![PathBuf::new()];
        for component in full.components() {
            match component {
                std::path::Component::Prefix(prefix) => {
                    paths = vec![PathBuf::from(prefix.as_os_str())];
                }
                std::path::Component::RootDir => {
                    if paths.len() == 1 && paths[0].as_os_str().is_empty() {
                        paths[0].push(std::path::MAIN_SEPARATOR.to_string());
                    } else {
                        for path in &mut paths {
                            path.push(std::path::MAIN_SEPARATOR.to_string());
                        }
                    }
                }
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    for path in &mut paths {
                        path.push("..");
                    }
                }
                std::path::Component::Normal(component) => {
                    let pattern = component.to_string_lossy().into_owned();
                    let recursive = self.env.option_enabled("globstar") && pattern == "**";
                    let patterned = contains_glob_meta(&pattern)
                        || find_extglob(&pattern).is_some();

                    if recursive {
                        let mut expanded = Vec::new();
                        for base in &paths {
                            expanded.push(base.clone());
                            descendants(
                                base,
                                self.env.option_enabled("dotglob")
                                    || !self.env.get("GLOBIGNORE").is_empty(),
                                &mut expanded,
                            );
                        }
                        paths = expanded;
                        continue;
                    }

                    if !patterned {
                        for path in &mut paths {
                            path.push(&pattern);
                        }
                        continue;
                    }

                    let mut expanded = Vec::new();
                    for base in &paths {
                        let directory = if base.as_os_str().is_empty() {
                            Path::new(".")
                        } else {
                            base.as_path()
                        };
                        let Ok(entries) = fs::read_dir(directory) else { continue };
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if name.starts_with('.')
                                && !self.env.option_enabled("dotglob")
                                && self.env.get("GLOBIGNORE").is_empty()
                                && !pattern.starts_with('.')
                            {
                                continue;
                            }
                            if shell_pattern_matches(
                                &pattern,
                                &name,
                                true,
                                self.env.option_enabled("nocaseglob"),
                            ) {
                                expanded.push(entry.path());
                            }
                        }
                    }
                    paths = expanded;
                }
            }
        }

        let mut result = paths.into_iter()
            .filter(|path| path.exists())
            .filter_map(|path| {
                if absolute {
                    Some(path.to_string_lossy().into_owned())
                } else {
                    path.strip_prefix(&self.env.cwd)
                        .ok()
                        .map(|relative| relative.to_string_lossy().into_owned())
                }
            })
            .collect::<Vec<_>>();
        result.dedup();
        Ok(self.finalize_glob_results(result))
    }

    fn finalize_glob_results(&self, mut values: Vec<String>) -> Vec<String> {
        let ignore = self.env.get("GLOBIGNORE");
        if !ignore.is_empty() {
            let patterns = ignore.split(':')
                .filter(|pattern| !pattern.is_empty())
                .collect::<Vec<_>>();
            let extglob = self.env.option_enabled("extglob");
            let nocase = self.env.option_enabled("nocaseglob");

            values.retain(|value| {
                let name = Path::new(value)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(value);
                if name == "." || name == ".." {
                    return false;
                }
                !patterns.iter().any(|pattern| {
                    let target = if pattern.contains('/') || pattern.contains('\\') {
                        value.as_str()
                    } else {
                        name
                    };
                    shell_pattern_matches(pattern, target, extglob, nocase)
                })
            });
        }

        self.sort_glob_results(&mut values);
        values
    }

    fn sort_glob_results(&self, values: &mut [String]) {
        let raw = self.env.get("GLOBSORT");
        let (descending, key) = match raw.as_str() {
            value if value.starts_with('-') => (true, &value[1..]),
            value if value.starts_with('+') => (false, &value[1..]),
            value => (false, value),
        };
        let key = if key.is_empty() { "name" } else { key };
        if key == "nosort" {
            return;
        }

        let valid = matches!(
            key,
            "name" | "numeric" | "size" | "mtime" | "atime" | "ctime" | "blocks"
        );
        let key = if valid { key } else { "name" };

        fn numeric_name(value: &str) -> Option<(usize, String)> {
            let name = Path::new(value).file_name()?.to_string_lossy();
            if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let normalized = name.trim_start_matches('0');
            let normalized = if normalized.is_empty() { "0" } else { normalized };
            Some((normalized.len(), normalized.to_owned()))
        }

        fn time_key(value: std::io::Result<std::time::SystemTime>) -> (u64, u32) {
            value.ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
                .unwrap_or((0, 0))
        }

        let cwd = self.env.cwd.clone();
        values.sort_by(|left, right| {
            let left_path = if Path::new(left).is_absolute() {
                PathBuf::from(left)
            } else {
                cwd.join(left)
            };
            let right_path = if Path::new(right).is_absolute() {
                PathBuf::from(right)
            } else {
                cwd.join(right)
            };

            let name_cmp = left.cmp(right);
            let order = match key {
                "numeric" => match (numeric_name(left), numeric_name(right)) {
                    (Some((ll, lv)), Some((rl, rv))) => ll.cmp(&rl).then_with(|| lv.cmp(&rv)).then(name_cmp),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => name_cmp,
                },
                "size" | "blocks" => {
                    let left_len = fs::metadata(&left_path).map(|metadata| metadata.len()).unwrap_or(0);
                    let right_len = fs::metadata(&right_path).map(|metadata| metadata.len()).unwrap_or(0);
                    let left_value = if key == "blocks" { (left_len + 511) / 512 } else { left_len };
                    let right_value = if key == "blocks" { (right_len + 511) / 512 } else { right_len };
                    left_value.cmp(&right_value).then(name_cmp)
                }
                "mtime" | "atime" | "ctime" => {
                    let left_meta = fs::metadata(&left_path);
                    let right_meta = fs::metadata(&right_path);
                    let left_time = match key {
                        "mtime" => left_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.modified()),
                        "atime" => left_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.accessed()),
                        // Windows has no POSIX inode-change time through std; creation
                        // time is the closest stable metadata timestamp available.
                        _ => left_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.created()),
                    };
                    let right_time = match key {
                        "mtime" => right_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.modified()),
                        "atime" => right_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.accessed()),
                        _ => right_meta.as_ref().map_err(|error| std::io::Error::new(error.kind(), error.to_string())).and_then(|metadata| metadata.created()),
                    };
                    time_key(left_time).cmp(&time_key(right_time)).then(name_cmp)
                }
                _ => name_cmp,
            };
            if descending { order.reverse() } else { order }
        });
    }

    fn resolve_source_path(&self, raw: &str, override_path: Option<&str>) -> PathBuf {
        let explicit = Path::new(raw).is_absolute()
            || raw.contains('/')
            || raw.contains('\\');
        if explicit {
            return self.resolve_path(raw);
        }

        let path_value = override_path
            .map(str::to_owned)
            .unwrap_or_else(|| self.env.get("PATH"));

        if override_path.is_some() || self.env.option_enabled("sourcepath") {
            for directory in std::env::split_paths(&path_value) {
                let candidate = directory.join(raw);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }

        self.resolve_path(raw)
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

fn set_shell_option(env: &mut ShellEnvironment, name: &str, enabled: bool) {
    if enabled { env.shell_options.insert(name.to_owned()); }
    else { env.shell_options.remove(name); }
}

fn normalize_signal(value: &str) -> String {
    let upper = value.trim_start_matches("SIG").to_ascii_uppercase();
    match upper.as_str() {
        "0" => "EXIT".to_owned(),
        "1" => "HUP".to_owned(),
        "2" => "INT".to_owned(),
        "3" => "QUIT".to_owned(),
        "15" => "TERM".to_owned(),
        other => other.to_owned(),
    }
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

fn basic_shell_pattern_matches(pattern: &str, value: &str, nocase: bool) -> bool {
    let Ok(pattern) = glob::Pattern::new(pattern) else {
        return if nocase {
            pattern.eq_ignore_ascii_case(value)
        } else {
            pattern == value
        };
    };
    pattern.matches_with(
        value,
        glob::MatchOptions {
            case_sensitive: !nocase,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        },
    )
}

fn find_extglob(pattern: &str) -> Option<(usize, char, usize, usize)> {
    let chars: Vec<(usize, char)> = pattern.char_indices().collect();
    let mut index = 0usize;
    let mut escaped = false;
    while index + 1 < chars.len() {
        let (byte, ch) = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            index += 1;
            continue;
        }
        if matches!(ch, '?' | '*' | '+' | '@' | '!')
            && chars[index + 1].1 == '('
        {
            let open_byte = chars[index + 1].0;
            let mut depth = 1usize;
            let mut cursor = index + 2;
            let mut inner_escaped = false;
            while cursor < chars.len() {
                let (close_byte, current) = chars[cursor];
                if inner_escaped {
                    inner_escaped = false;
                    cursor += 1;
                    continue;
                }
                if current == '\\' {
                    inner_escaped = true;
                    cursor += 1;
                    continue;
                }
                if current == '(' {
                    depth += 1;
                } else if current == ')' {
                    depth -= 1;
                    if depth == 0 {
                        return Some((byte, ch, open_byte, close_byte));
                    }
                }
                cursor += 1;
            }
            return None;
        }
        index += 1;
    }
    None
}

fn split_extglob_alternatives(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut escaped = false;
    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&body[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

fn extglob_repetition_matches(
    alternatives: &[&str],
    value: &str,
    minimum: usize,
    nocase: bool,
) -> bool {
    if value.is_empty() {
        if minimum == 0 {
            return true;
        }
        return alternatives.iter().any(|pattern| {
            shell_pattern_matches(pattern, "", true, nocase)
        });
    }

    let boundaries = char_boundaries(value);
    let mut stack = vec![(0usize, 0usize)];
    let mut visited = HashSet::new();

    while let Some((start, count)) = stack.pop() {
        if !visited.insert((start, count.min(minimum))) {
            continue;
        }
        if start == value.len() && count >= minimum {
            return true;
        }
        for &end in boundaries.iter().filter(|&&end| end > start) {
            let segment = &value[start..end];
            if alternatives.iter().any(|pattern| {
                shell_pattern_matches(pattern, segment, true, nocase)
            }) {
                stack.push((end, count + 1));
            }
        }
    }

    false
}

fn shell_pattern_matches(pattern: &str, value: &str, extglob: bool, nocase: bool) -> bool {
    if !extglob {
        return basic_shell_pattern_matches(pattern, value, nocase);
    }

    let Some((start, operator, open, close)) = find_extglob(pattern) else {
        return basic_shell_pattern_matches(pattern, value, nocase);
    };

    let prefix = &pattern[..start];
    let body = &pattern[open + 1..close];
    let suffix = &pattern[close + 1..];
    let alternatives = split_extglob_alternatives(body);
    let boundaries = char_boundaries(value);

    for &prefix_end in &boundaries {
        if !basic_shell_pattern_matches(prefix, &value[..prefix_end], nocase) {
            continue;
        }

        for &group_end in boundaries.iter().filter(|&&end| end >= prefix_end) {
            let segment = &value[prefix_end..group_end];
            let group_matches = match operator {
                '@' => alternatives.iter().any(|pattern| {
                    shell_pattern_matches(pattern, segment, true, nocase)
                }),
                '?' => segment.is_empty() || alternatives.iter().any(|pattern| {
                    shell_pattern_matches(pattern, segment, true, nocase)
                }),
                '*' => extglob_repetition_matches(&alternatives, segment, 0, nocase),
                '+' => extglob_repetition_matches(&alternatives, segment, 1, nocase),
                '!' => !alternatives.iter().any(|pattern| {
                    shell_pattern_matches(pattern, segment, true, nocase)
                }),
                _ => false,
            };

            if group_matches
                && shell_pattern_matches(suffix, &value[group_end..], true, nocase)
            {
                return true;
            }
        }
    }

    false
}

fn replacement_text(replacement: &str, matched: &str, expand_ampersand: bool) -> String {
    if !expand_ampersand {
        return replacement.to_owned();
    }
    let mut output = String::new();
    let mut escaped = false;
    for ch in replacement.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '&' {
            output.push_str(matched);
        } else {
            output.push(ch);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn remove_glob_pattern(
    value: &str,
    pattern: &str,
    operator: &str,
    extglob: bool,
) -> String {
    let points = char_boundaries(value);
    match operator {
        "#" | "##" => {
            let ordered: Vec<usize> = if operator == "#" {
                points.clone()
            } else {
                points.iter().copied().rev().collect()
            };
            for point in ordered {
                if shell_pattern_matches(pattern, &value[..point], extglob, false) {
                    return value[point..].to_owned();
                }
            }
        }
        "%" | "%%" => {
            let ordered: Vec<usize> = if operator == "%" {
                points.iter().copied().rev().collect()
            } else {
                points.clone()
            };
            for point in ordered {
                if shell_pattern_matches(pattern, &value[point..], extglob, false) {
                    return value[..point].to_owned();
                }
            }
        }
        _ => {}
    }
    value.to_owned()
}

fn replace_glob(
    value: &str,
    pattern: &str,
    replacement: &str,
    all: bool,
    extglob: bool,
    expand_ampersand: bool,
) -> String {
    let points = char_boundaries(value);
    let mut out = String::new();
    let mut cursor = 0usize;

    while cursor < value.len() {
        let mut found = None;
        'outer: for &start in points.iter().filter(|&&point| point >= cursor) {
            for &end in points.iter().filter(|&&point| point > start) {
                if shell_pattern_matches(pattern, &value[start..end], extglob, false) {
                    found = Some((start, end));
                    break 'outer;
                }
            }
        }
        let Some((start, end)) = found else {
            out.push_str(&value[cursor..]);
            break;
        };
        out.push_str(&value[cursor..start]);
        out.push_str(&replacement_text(
            replacement,
            &value[start..end],
            expand_ampersand,
        ));
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
    extglob: bool,
    expand_ampersand: bool,
) -> String {
    let points = char_boundaries(value);
    if prefix {
        for &end in points.iter().rev() {
            if shell_pattern_matches(pattern, &value[..end], extglob, false) {
                return format!(
                    "{}{}",
                    replacement_text(replacement, &value[..end], expand_ampersand),
                    &value[end..],
                );
            }
        }
    } else {
        for &start in &points {
            if shell_pattern_matches(pattern, &value[start..], extglob, false) {
                return format!(
                    "{}{}",
                    &value[..start],
                    replacement_text(replacement, &value[start..], expand_ampersand),
                );
            }
        }
    }
    value.to_owned()
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
            ArithmeticToken::Ident(name) => Ok(self.env.get(&name).parse::<i64>().unwrap_or(0)),
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
        RedirectKind::Dup => ">&",
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
                    rendered.push_str(if stderr_to_pipe.get(index - 1).copied().unwrap_or(false) {
                        " |& "
                    } else {
                        " | "
                    });
                }
                rendered.push_str(&render_ast(part));
            }
            rendered
        },
        AstNode::Time { body, posix } => format!(
            "time {}{}",
            if *posix { "-p " } else { "" },
            render_ast(body),
        ),
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
