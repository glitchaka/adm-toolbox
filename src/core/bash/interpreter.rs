use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};

use super::{
    ast::{AstNode, RedirectKind, SimpleCommand},
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
}

pub struct Interpreter {
    pub env: ShellEnvironment,
    host: Box<dyn ShellCommandHost>,
    loop_depth: usize,
}

impl Interpreter {
    pub fn new(host: Box<dyn ShellCommandHost>) -> Self {
        Self { env: ShellEnvironment::new(), host, loop_depth: 0 }
    }

    pub fn execute_text(&mut self, input: &str) -> Result<ExecutionResult> {
        let node = parse(input)?;
        self.execute(&node, None)
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
            AstNode::Pipeline(parts) => self.execute_pipeline(parts, stdin)?,
            AstNode::Simple(command) => self.execute_simple(command, stdin)?,
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
                for arm in arms {
                    let mut matched = false;
                    for pattern in &arm.patterns {
                        let pattern = self.expand_scalar(pattern)?;
                        if glob::Pattern::new(&pattern)
                            .map(|candidate| candidate.matches(&value))
                            .unwrap_or(false)
                        {
                            matched = true;
                            break;
                        }
                    }
                    if matched {
                        result = self.execute(&arm.body, stdin)?;
                        break;
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

    fn execute_pipeline(&mut self, parts: &[AstNode], stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut input = stdin.map(ToOwned::to_owned);
        let mut stderr = String::new();
        let mut last = ExecutionResult::success();

        for part in parts {
            let saved = self.env.clone();
            let result = self.execute(part, input.as_deref());
            self.env = saved;
            last = result?;
            last.exit_requested = false;
            last.flow = FlowSignal::None;
            stderr.push_str(&last.stderr);
            input = Some(last.stdout.as_bytes().to_vec());
        }

        last.stderr = stderr;
        Ok(last)
    }

    fn execute_simple(&mut self, command: &SimpleCommand, stdin: Option<&[u8]>) -> Result<ExecutionResult> {
        let mut local_stdin = stdin.map(ToOwned::to_owned);

        for redirect in &command.redirects {
            match redirect.kind {
                RedirectKind::Read => {
                    let target = self.expand_scalar(&redirect.target)?;
                    local_stdin = Some(fs::read(self.resolve_path(&target))?);
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
            return Ok(ExecutionResult::success());
        }

        let words = self.expand_words(&raw[index..])?;
        if words.is_empty() {
            return Ok(ExecutionResult::success());
        }

        let name = words[0].clone();
        let args = &words[1..];

        let mut result = if let Some(result) = self.shell_builtin(&name, args, local_stdin.as_deref())? {
            result
        } else if let Some(body) = self.env.functions.get(&name).cloned() {
            let saved = self.env.positional.clone();
            self.env.positional = args.to_vec();
            self.env.push_local_scope();
            let execution = self.execute(&body, local_stdin.as_deref());
            self.env.pop_local_scope();
            self.env.positional = saved;
            let mut result = execution?;
            if result.flow == FlowSignal::Return {
                result.flow = FlowSignal::None;
            }
            result
        } else if let Some(result) = self.host.execute_builtin(
            &name,
            args,
            &self.env.cwd,
            local_stdin.as_deref(),
        )? {
            result
        } else {
            match self.host.execute_external(
                &name,
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

        self.apply_output_redirects(command, &mut result)?;
        Ok(result)
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
                let path = self.resolve_path(raw);
                match fs::canonicalize(path) {
                    Ok(path) if path.is_dir() => {
                        self.env.cwd = path;
                        ExecutionResult::success()
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
            "export" => {
                for arg in args {
                    if let Some((name, value)) = arg.split_once('=') {
                        let value = self.expand_scalar(value)?;
                        self.env.export(name.to_owned(), value);
                    } else {
                        let value = self.env.get(arg);
                        self.env.export(arg.clone(), value);
                    }
                }
                ExecutionResult::success()
            }
            "unset" => {
                for name in args {
                    self.env.vars.remove(name);
                    self.env.exported.remove(name);
                    self.env.functions.remove(name);
                }
                ExecutionResult::success()
            }
            "alias" => {
                if args.is_empty() {
                    let mut aliases = self.env.aliases.iter().collect::<Vec<_>>();
                    aliases.sort_by_key(|(name, _)| *name);
                    let stdout = aliases.into_iter()
                        .map(|(name, value)| format!("alias {name}='{value}'\n"))
                        .collect();
                    ExecutionResult::from_parts(stdout, String::new(), 0)
                } else {
                    let mut stderr = String::new();
                    let mut status = 0;
                    for arg in args {
                        if let Some((name, value)) = arg.split_once('=') {
                            self.env.aliases.insert(name.to_owned(), value.to_owned());
                        } else if let Some(value) = self.env.aliases.get(arg) {
                            stderr.push_str(&format!("alias {arg}='{value}'\n"));
                        } else {
                            stderr.push_str(&format!("alias: {arg}: no encontrado\n"));
                            status = 1;
                        }
                    }
                    ExecutionResult::from_parts(String::new(), stderr, status)
                }
            }
            "unalias" => {
                for name in args {
                    self.env.aliases.remove(name);
                }
                ExecutionResult::success()
            }
            "exit" => {
                let status = args.first()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(self.env.last_status);
                ExecutionResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    status,
                    exit_requested: true,
                    flow: FlowSignal::None,
                }
            }
            "source" | "." => {
                let Some(path) = args.first() else {
                    return Ok(Some(ExecutionResult::from_parts(
                        String::new(),
                        format!("{name}: falta archivo\n"),
                        2,
                    )));
                };
                let source = fs::read_to_string(self.resolve_path(path))?;
                self.execute_text(&source)?
            }
            "read" => {
                let variable = args.last().cloned().unwrap_or_else(|| "REPLY".to_owned());
                let value = stdin
                    .map(|bytes| String::from_utf8_lossy(bytes).lines().next().unwrap_or("").to_owned())
                    .unwrap_or_default();
                self.env.set(variable, value);
                ExecutionResult::success()
            }
            "local" => {
                if self.env.local_scopes.is_empty() {
                    ExecutionResult::from_parts(
                        String::new(),
                        "local: solo puede usarse dentro de una función\n".to_owned(),
                        1,
                    )
                } else {
                    for arg in args {
                        if let Some((name, value)) = arg.split_once('=') {
                            let value = self.expand_scalar(value)?;
                            self.env.set_local(name.to_owned(), value);
                        } else {
                            let value = self.env.get(arg);
                            self.env.set_local(arg.clone(), value);
                        }
                    }
                    ExecutionResult::success()
                }
            }
            "break" => {
                if self.loop_depth == 0 {
                    ExecutionResult::from_parts(
                        String::new(),
                        "break: solo puede usarse dentro de un bucle\n".to_owned(),
                        1,
                    )
                } else {
                    let levels = args.first().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                    let mut result = ExecutionResult::success();
                    result.flow = FlowSignal::Break(levels.min(self.loop_depth));
                    result
                }
            }
            "continue" => {
                if self.loop_depth == 0 {
                    ExecutionResult::from_parts(
                        String::new(),
                        "continue: solo puede usarse dentro de un bucle\n".to_owned(),
                        1,
                    )
                } else {
                    let levels = args.first().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
                    let mut result = ExecutionResult::success();
                    result.flow = FlowSignal::Continue(levels.min(self.loop_depth));
                    result
                }
            }
            "return" => {
                let status = args.first()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(self.env.last_status);
                if self.env.local_scopes.is_empty() {
                    ExecutionResult::from_parts(
                        String::new(),
                        "return: solo puede usarse dentro de una función\n".to_owned(),
                        1,
                    )
                } else {
                    let mut result = ExecutionResult::from_parts(String::new(), String::new(), status);
                    result.flow = FlowSignal::Return;
                    result
                }
            }
            "xargs" => {
                self.execute_xargs(args, stdin)?
            }
            "config" => {
                match args.first().map(String::as_str).unwrap_or("path") {
                    "path" => ExecutionResult::from_parts(
                        format!("{}\n", self.env.get("ADM_CONFIG")),
                        String::new(),
                        0,
                    ),
                    "reload" => {
                        let path = self.env.get("ADM_CONFIG");
                        if path.is_empty() {
                            ExecutionResult::from_parts(
                                String::new(),
                                "config: ADM_CONFIG no definido\n".to_owned(),
                                1,
                            )
                        } else {
                            self.execute_text(&fs::read_to_string(path)?)?
                        }
                    }
                    "edit" => {
                        let edit_args = vec!["edit".to_owned()];
                        self.host.execute_builtin(
                            "adm-config",
                            &edit_args,
                            &self.env.cwd,
                            stdin,
                        )?.unwrap_or_else(|| ExecutionResult::from_parts(
                            String::new(),
                            "config edit: builtin no disponible\n".to_owned(),
                            127,
                        ))
                    }
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };

        Ok(Some(result))
    }


    fn evaluate_conditional(&mut self, expression: &[String]) -> Result<bool> {
        fn split_top_level<'a>(items: &'a [String], operator: &str) -> Option<(&'a [String], &'a [String])> {
            let mut depth = 0i32;
            for (index, item) in items.iter().enumerate() {
                match item.as_str() {
                    "(" => depth += 1,
                    ")" => depth -= 1,
                    _ if depth == 0 && item == operator => return Some((&items[..index], &items[index + 1..])),
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
            [value] => Ok(!self.expand_scalar(value)?.is_empty()),
            [op, value] => {
                let value = self.expand_scalar(value)?;
                Ok(match op.as_str() {
                    "-n" => !value.is_empty(),
                    "-z" => value.is_empty(),
                    "-e" => self.resolve_path(&value).exists(),
                    "-f" => self.resolve_path(&value).is_file(),
                    "-d" => self.resolve_path(&value).is_dir(),
                    _ => false,
                })
            }
            [left, op, right] => {
                let left = self.expand_scalar(left)?;
                let right = self.expand_scalar(right)?;
                Ok(match op.as_str() {
                    "=" | "==" => glob::Pattern::new(&right)
                        .map(|pattern| pattern.matches(&left))
                        .unwrap_or(left == right),
                    "!=" => glob::Pattern::new(&right)
                        .map(|pattern| !pattern.matches(&left))
                        .unwrap_or(left != right),
                    "<" => left < right,
                    ">" => left > right,
                    "-eq" => left.parse::<i64>().unwrap_or(0) == right.parse::<i64>().unwrap_or(0),
                    "-ne" => left.parse::<i64>().unwrap_or(0) != right.parse::<i64>().unwrap_or(0),
                    "-lt" => left.parse::<i64>().unwrap_or(0) < right.parse::<i64>().unwrap_or(0),
                    "-le" => left.parse::<i64>().unwrap_or(0) <= right.parse::<i64>().unwrap_or(0),
                    "-gt" => left.parse::<i64>().unwrap_or(0) > right.parse::<i64>().unwrap_or(0),
                    "-ge" => left.parse::<i64>().unwrap_or(0) >= right.parse::<i64>().unwrap_or(0),
                    _ => false,
                })
            }
            _ => Ok(false),
        }
    }

    fn evaluate_arithmetic_command(&mut self, expression: &str) -> Result<i64> {
        let expression = expression.trim();

        for suffix in ["++", "--"] {
            if let Some(name) = expression.strip_suffix(suffix).map(str::trim) {
                if is_variable_name(name) {
                    let current = self.env.get(name).parse::<i64>().unwrap_or(0);
                    let next = if suffix == "++" { current + 1 } else { current - 1 };
                    self.env.set(name.to_owned(), next.to_string());
                    return Ok(current);
                }
            }
        }

        for prefix in ["++", "--"] {
            if let Some(name) = expression.strip_prefix(prefix).map(str::trim) {
                if is_variable_name(name) {
                    let current = self.env.get(name).parse::<i64>().unwrap_or(0);
                    let next = if prefix == "++" { current + 1 } else { current - 1 };
                    self.env.set(name.to_owned(), next.to_string());
                    return Ok(next);
                }
            }
        }

        for operator in ["==", "!=", ">=", "<=", ">", "<"] {
            if let Some((left, right)) = expression.split_once(operator) {
                let left = eval_arithmetic(left.trim(), &self.env)?;
                let right = eval_arithmetic(right.trim(), &self.env)?;
                return Ok(match operator {
                    "==" => (left == right) as i64,
                    "!=" => (left != right) as i64,
                    ">=" => (left >= right) as i64,
                    "<=" => (left <= right) as i64,
                    ">" => (left > right) as i64,
                    "<" => (left < right) as i64,
                    _ => 0,
                });
            }
        }

        for operator in ["+=", "-=", "*=", "/=", "%="] {
            if let Some((name, rhs)) = expression.split_once(operator) {
                let name = name.trim();
                if is_variable_name(name) {
                    let right = eval_arithmetic(rhs.trim(), &self.env)?;
                    let current = self.env.get(name).parse::<i64>().unwrap_or(0);
                    let value = match operator {
                        "+=" => current + right,
                        "-=" => current - right,
                        "*=" => current * right,
                        "/=" => if right == 0 { 0 } else { current / right },
                        "%=" => if right == 0 { 0 } else { current % right },
                        _ => unreachable!(),
                    };
                    self.env.set(name.to_owned(), value.to_string());
                    return Ok(value);
                }
            }
        }

        if expression.matches('=').count() == 1 {
            if let Some((name, rhs)) = expression.split_once('=') {
                let name = name.trim();
                if is_variable_name(name) {
                    let value = eval_arithmetic(rhs.trim(), &self.env)?;
                    self.env.set(name.to_owned(), value.to_string());
                    return Ok(value);
                }
            }
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
                RedirectKind::Write | RedirectKind::Append => {
                    let target = self.expand_scalar(&redirect.target)?;
                    let path = self.resolve_path(&target);
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
                RedirectKind::Dup => {
                    let target = self.expand_scalar(&redirect.target)?;
                    if redirect.fd == 2 && target == "1" {
                        result.stdout.push_str(&result.stderr);
                        result.stderr.clear();
                    } else if redirect.fd == 1 && target == "2" {
                        result.stderr.push_str(&result.stdout);
                        result.stdout.clear();
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn expand_words(&mut self, words: &[String]) -> Result<Vec<String>> {
        let mut result = Vec::new();

        for raw in words {
            for braced in brace_expand(raw) {
                let quoted = braced.contains('\'') || braced.contains('"');
                let expanded = self.expand_scalar(&braced)?;

                if quoted {
                    result.push(expanded);
                    continue;
                }

                for field in expanded.split_whitespace() {
                    let paths = self.glob(field)?;
                    if paths.is_empty() {
                        result.push(field.to_owned());
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
            match chars[i] {
                '\'' if !double => { single = !single; i += 1; }
                '"' if !single => { double = !double; i += 1; }
                '\\' if !single && i + 1 < chars.len() => {
                    i += 1;
                    out.push(chars[i]);
                    i += 1;
                }
                '$' if !single => {
                    if chars.get(i + 1) == Some(&'(') && chars.get(i + 2) == Some(&'(') {
                        let end = arithmetic_end(&chars, i + 3)
                            .ok_or_else(|| anyhow!("expansión aritmética sin cerrar"))?;
                        let expression: String = chars[i + 3..end].iter().collect();
                        out.push_str(&eval_arithmetic(&expression, &self.env)?.to_string());
                        i = end + 2;
                    } else if chars.get(i + 1) == Some(&'(') {
                        let end = matching(&chars, i + 1, '(', ')')
                            .ok_or_else(|| anyhow!("sustitución de comando sin cerrar"))?;
                        let source: String = chars[i + 2..end].iter().collect();
                        let saved = self.env.clone();
                        let result = self.execute_text(&source);
                        self.env = saved;
                        let result = result?;
                        out.push_str(result.stdout.trim_end_matches(['\r', '\n']));
                        i = end + 1;
                    } else if chars.get(i + 1) == Some(&'{') {
                        let end = matching(&chars, i + 1, '{', '}')
                            .ok_or_else(|| anyhow!("expansión de parámetro sin cerrar"))?;
                        let expression: String = chars[i + 2..end].iter().collect();
                        out.push_str(&parameter(&self.env, &expression));
                        i = end + 1;
                    } else {
                        let (name, used) = parameter_name(&chars[i + 1..]);
                        if used == 0 {
                            out.push('$');
                            i += 1;
                        } else {
                            out.push_str(&self.env.get(&name));
                            i += used + 1;
                        }
                    }
                }
                ch => { out.push(ch); i += 1; }
            }
        }

        if single || double {
            bail!("comillas sin cerrar");
        }
        Ok(out)
    }

    fn tilde_expand(&self, raw: &str) -> String {
        if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
            let home = self.env.get("USERPROFILE");
            if !home.is_empty() {
                return format!("{home}{}", &raw[1..]);
            }
        }
        raw.to_owned()
    }

    fn glob(&self, value: &str) -> Result<Vec<String>> {
        if !value.contains(['*', '?', '[']) {
            return Ok(Vec::new());
        }
        let pattern = if Path::new(value).is_absolute() {
            value.to_owned()
        } else {
            self.env.cwd.join(value).to_string_lossy().into_owned()
        };
        let mut result = Vec::new();
        for entry in glob::glob(&pattern)? {
            let Ok(path) = entry else { continue };
            if Path::new(value).is_absolute() {
                result.push(path.to_string_lossy().into_owned());
            } else if let Ok(relative) = path.strip_prefix(&self.env.cwd) {
                result.push(relative.to_string_lossy().into_owned());
            }
        }
        result.sort();
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


fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }

    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\' | ':' | '@' | '%')
    }) {
        return value.to_owned();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn is_variable_name(name: &str) -> bool {
    is_variable_name(name)
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else { return false };
    !name.is_empty() && name.chars().enumerate().all(|(index, ch)| {
        ch == '_' || (ch.is_ascii_alphanumeric() && (index > 0 || !ch.is_ascii_digit()))
    })
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
                    if inner.contains(',') {
                        let prefix: String = chars[..start].iter().collect();
                        let suffix: String = chars[index + 1..].iter().collect();
                        return inner.split(',')
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
        if matches!(ch, '?' | '#' | '@' | '*') || ch.is_ascii_digit() {
            return (ch.to_string(), 1);
        }
    }
    let mut len = 0usize;
    for ch in chars {
        if *ch == '_' || ch.is_ascii_alphanumeric() { len += 1; } else { break; }
    }
    (chars[..len].iter().collect(), len)
}

fn parameter(env: &ShellEnvironment, expression: &str) -> String {
    if let Some(name) = expression.strip_prefix('#') {
        return env.get(name).chars().count().to_string();
    }
    for operator in [":-", ":+", ":=", ":?"] {
        if let Some((name, alternative)) = expression.split_once(operator) {
            let value = env.get(name);
            return match operator {
                ":-" => if value.is_empty() { alternative.to_owned() } else { value },
                ":+" => if value.is_empty() { String::new() } else { alternative.to_owned() },
                ":=" => if value.is_empty() { alternative.to_owned() } else { value },
                ":?" => if value.is_empty() { String::new() } else { value },
                _ => value,
            };
        }
    }
    env.get(expression)
}

fn eval_arithmetic(expression: &str, env: &ShellEnvironment) -> Result<i64> {
    #[derive(Clone, Copy)]
    enum Token { Value(i64), Op(char), LParen, RParen }

    fn precedence(op: char) -> u8 {
        match op { '+' | '-' => 1, '*' | '/' | '%' => 2, _ => 0 }
    }

    fn apply(values: &mut Vec<i64>, op: char) -> Result<()> {
        let right = values.pop().ok_or_else(|| anyhow!("expresión aritmética inválida"))?;
        let left = values.pop().unwrap_or(0);
        values.push(match op {
            '+' => left + right,
            '-' => left - right,
            '*' => left * right,
            '/' => if right == 0 { 0 } else { left / right },
            '%' => if right == 0 { 0 } else { left % right },
            _ => bail!("operador aritmético inválido"),
        });
        Ok(())
    }

    let chars: Vec<char> = expression.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index].is_whitespace() { index += 1; continue; }
        if chars[index].is_ascii_digit() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_digit() { index += 1; }
            tokens.push(Token::Value(chars[start..index].iter().collect::<String>().parse()?));
            continue;
        }
        if chars[index] == '_' || chars[index].is_ascii_alphabetic() {
            let start = index;
            while index < chars.len()
                && (chars[index] == '_' || chars[index].is_ascii_alphanumeric()) { index += 1; }
            let name: String = chars[start..index].iter().collect();
            tokens.push(Token::Value(env.get(&name).parse().unwrap_or(0)));
            continue;
        }
        tokens.push(match chars[index] {
            '+' | '-' | '*' | '/' | '%' => Token::Op(chars[index]),
            '(' => Token::LParen,
            ')' => Token::RParen,
            ch => bail!("operador aritmético no soportado: {ch}"),
        });
        index += 1;
    }

    let mut values = Vec::new();
    let mut operators = Vec::new();
    for token in tokens {
        match token {
            Token::Value(value) => values.push(value),
            Token::LParen => operators.push(Token::LParen),
            Token::RParen => {
                while let Some(token) = operators.pop() {
                    match token {
                        Token::LParen => break,
                        Token::Op(op) => apply(&mut values, op)?,
                        _ => {}
                    }
                }
            }
            Token::Op(op) => {
                while let Some(Token::Op(top)) = operators.last().copied() {
                    if precedence(top) < precedence(op) { break; }
                    operators.pop();
                    apply(&mut values, top)?;
                }
                operators.push(Token::Op(op));
            }
        }
    }
    while let Some(token) = operators.pop() {
        if let Token::Op(op) = token { apply(&mut values, op)?; }
    }
    Ok(values.pop().unwrap_or(0))
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
