use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};

use crate::{
    builtins::CommandRegistry,
    core::{
        CommandContext,
        ShellExecution,
        bash::{CoprocHandles, ExecutionResult, Interpreter, JobInfo, ShellCommandHost},
        ports::ShellEngine,
    },
};

struct BackgroundJob {
    command: String,
    child: Child,
}

struct WindowsShellHost {
    registry: Arc<CommandRegistry>,
    interrupt: Arc<std::sync::atomic::AtomicBool>,
    force_abort: Arc<std::sync::atomic::AtomicBool>,
    jobs: Mutex<HashMap<u32, BackgroundJob>>,
}

impl ShellCommandHost for WindowsShellHost {
    fn interrupted(&self) -> bool { self.interrupt.load(std::sync::atomic::Ordering::SeqCst) }

    fn clear_interrupt(&self) {
        self.interrupt.store(false, std::sync::atomic::Ordering::SeqCst);
        self.force_abort.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    fn read_line(&self, prompt: &str, silent: bool) -> Result<Option<String>> {
        use crossterm::event::{Event, KeyCode, KeyModifiers};

        crate::adapters::terminal::io::write(prompt.as_bytes())?;
        crate::adapters::terminal::io::enter_raw()?;
        struct RawGuard;
        impl Drop for RawGuard {
            fn drop(&mut self) { crate::adapters::terminal::io::leave_raw(); }
        }
        let _guard = RawGuard;

        let mut line = String::new();
        loop {
            match crate::adapters::terminal::io::read()? {
                Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('d') && line.is_empty() =>
                {
                    return Ok(None);
                }
                Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c') =>
                {
                    crate::adapters::terminal::io::write(b"^C\r\n")?;
                    return Ok(None);
                }
                Event::Key(key) => match key.code {
                    KeyCode::Enter => {
                        crate::adapters::terminal::io::write(b"\r\n")?;
                        return Ok(Some(line));
                    }
                    KeyCode::Backspace => {
                        if line.pop().is_some() && !silent {
                            crate::adapters::terminal::io::write(b"\x08 \x08")?;
                        }
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        line.push(ch);
                        if !silent {
                            let mut buf = [0u8; 4];
                            crate::adapters::terminal::io::write(ch.encode_utf8(&mut buf).as_bytes())?;
                        }
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    line.push_str(&text);
                    if !silent { crate::adapters::terminal::io::write(text.as_bytes())?; }
                }
                _ => {}
            }
        }
    }
    fn read_line_with_options(
        &self,
        prompt: &str,
        silent: bool,
        initial: &str,
        timeout: Option<std::time::Duration>,
        delimiter: Option<char>,
        max_chars: Option<usize>,
        exact_chars: bool,
    ) -> Result<Option<String>> {
        use crossterm::event::{Event, KeyCode, KeyModifiers};

        crate::adapters::terminal::io::write(prompt.as_bytes())?;
        crate::adapters::terminal::io::enter_raw()?;
        struct RawGuard;
        impl Drop for RawGuard {
            fn drop(&mut self) { crate::adapters::terminal::io::leave_raw(); }
        }
        let _guard = RawGuard;

        let mut line = initial.to_owned();
        if !silent && !initial.is_empty() {
            crate::adapters::terminal::io::write(initial.as_bytes())?;
        }
        if max_chars == Some(0) {
            return Ok(Some(String::new()));
        }
        if max_chars.is_some_and(|max| line.chars().count() >= max) {
            return Ok(Some(line.chars().take(max_chars.unwrap()).collect()));
        }

        let started = std::time::Instant::now();
        loop {
            if let Some(limit) = timeout {
                let elapsed = started.elapsed();
                if elapsed >= limit {
                    return Ok(None);
                }
                if !crate::adapters::terminal::io::poll(limit - elapsed)? {
                    return Ok(None);
                }
            }

            match crate::adapters::terminal::io::read()? {
                Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('d') && line.is_empty() =>
                {
                    return Ok(None);
                }
                Event::Key(key) if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c') =>
                {
                    crate::adapters::terminal::io::write(b"^C\r\n")?;
                    return Ok(None);
                }
                Event::Key(key) => match key.code {
                    KeyCode::Enter => {
                        if exact_chars {
                            line.push('\n');
                            if !silent {
                                crate::adapters::terminal::io::write(b"\r\n")?;
                            }
                            if max_chars.is_some_and(|max| line.chars().count() >= max) {
                                return Ok(Some(line.chars().take(max_chars.unwrap()).collect()));
                            }
                        } else if delimiter.is_none() || delimiter == Some('\n') {
                            crate::adapters::terminal::io::write(b"\r\n")?;
                            return Ok(Some(line));
                        }
                    }
                    KeyCode::Backspace => {
                        if line.pop().is_some() && !silent {
                            crate::adapters::terminal::io::write(b"\x08 \x08")?;
                        }
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if !exact_chars && delimiter == Some(ch) {
                            if !silent {
                                let mut buf = [0u8; 4];
                                crate::adapters::terminal::io::write(ch.encode_utf8(&mut buf).as_bytes())?;
                                crate::adapters::terminal::io::write(b"\r\n")?;
                            }
                            return Ok(Some(line));
                        }

                        line.push(ch);
                        if !silent {
                            let mut buf = [0u8; 4];
                            crate::adapters::terminal::io::write(ch.encode_utf8(&mut buf).as_bytes())?;
                        }
                        if max_chars.is_some_and(|max| line.chars().count() >= max) {
                            return Ok(Some(line.chars().take(max_chars.unwrap()).collect()));
                        }
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    for ch in text.chars() {
                        if !exact_chars && delimiter == Some(ch) {
                            return Ok(Some(line));
                        }
                        line.push(ch);
                        if max_chars.is_some_and(|max| line.chars().count() >= max) {
                            break;
                        }
                    }
                    if !silent {
                        crate::adapters::terminal::io::write(text.as_bytes())?;
                    }
                    if max_chars.is_some_and(|max| line.chars().count() >= max) {
                        return Ok(Some(line.chars().take(max_chars.unwrap()).collect()));
                    }
                }
                _ => {}
            }
        }
    }

    fn execute_builtin(
        &self,
        name: &str,
        args: &[String],
        cwd: &Path,
        stdin: Option<&[u8]>,
    ) -> Result<Option<ExecutionResult>> {
        if name == "help" || name == "man" {
            let output = self.registry.help(args.first().map(String::as_str));
            return Ok(Some(ExecutionResult::from_parts(
                output.stdout,
                output.stderr,
                output.status,
            )));
        }

        if !self.registry.names().iter().any(|candidate| candidate == name) {
            return Ok(None);
        }

        let output = self.registry.execute(name, args, CommandContext { cwd, stdin })?;
        Ok(Some(ExecutionResult::from_parts(
            output.stdout,
            output.stderr,
            output.status,
        )))
    }

    fn command_is_builtin(&self, name: &str) -> bool {
        name == "help" || name == "man" || self.registry.names().iter().any(|candidate| candidate == name)
    }

    fn command_names(&self) -> Vec<String> {
        let mut names = self.registry.names();
        names.extend(["help".to_owned(), "man".to_owned()]);
        names.sort();
        names.dedup();
        names
    }

    fn execute_external(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
        stdin: Option<&[u8]>,
    ) -> Result<ExecutionResult> {
        let script = resolve_shell_script(program, cwd);
        let mut command = if let Some(script) = &script {
            let mut command = Command::new(std::env::current_exe()?);
            command.arg(script);
            command
        } else {
            Command::new(program)
        };
        command
            .args(args)
            .current_dir(cwd)
            .envs(env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if stdin.is_some() {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::inherit());
        }

        let mut child = command.spawn()
            .with_context(|| format!("no se pudo ejecutar {program}"))?;

        if let (Some(bytes), Some(mut writer)) = (stdin, child.stdin.take()) {
            writer.write_all(bytes)?;
        }

        loop {
            if self.force_abort.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(ExecutionResult::from_parts(
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                    130,
                ));
            }

            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(ExecutionResult::from_parts(
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                    output.status.code().unwrap_or(1),
                ));
            }

            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn execute_shell_pipeline(
        &self,
        sources: &[String],
        stderr_to_pipe: &[bool],
        cwd: &Path,
        env: &HashMap<String, String>,
        stdin: Option<&[u8]>,
    ) -> Result<Option<(ExecutionResult, Vec<i32>)>> {
        if sources.len() < 2 {
            return Ok(None);
        }

        fn shell_quote(value: &str) -> String {
            format!("'{}'", value.replace('\'', "'\\''"))
        }

        let executable = std::env::current_exe()?;
        let mut children = Vec::with_capacity(sources.len());
        let mut previous_stdout = None;
        let mut stderr_readers = Vec::with_capacity(sources.len());
        let mut final_stdout_reader = None;
        let mut stdin_writer = None;

        for (index, source) in sources.iter().enumerate() {
            let stage_source = if stderr_to_pipe.get(index).copied().unwrap_or(false) {
                // Bash defines `cmd |& next` as `cmd 2>&1 | next`. Running the
                // stage through eval lets the shell perform that merge before its
                // stdout is connected to the next OS pipe.
                format!("eval {} 2>&1", shell_quote(source))
            } else {
                source.clone()
            };
            let mut command = Command::new(&executable);
            command
                .arg("-c")
                .arg(stage_source)
                .current_dir(cwd)
                .envs(env)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if index == 0 {
                if stdin.is_some() {
                    command.stdin(Stdio::piped());
                } else {
                    command.stdin(Stdio::inherit());
                }
            } else {
                let upstream = previous_stdout.take()
                    .ok_or_else(|| anyhow::anyhow!("pipeline: stdout upstream no disponible"))?;
                command.stdin(Stdio::from(upstream));
            }

            let mut child = command.spawn()
                .with_context(|| format!("no se pudo lanzar etapa {} del pipeline", index + 1))?;

            if index == 0 {
                if let (Some(bytes), Some(mut writer)) = (stdin, child.stdin.take()) {
                    let data = bytes.to_vec();
                    stdin_writer = Some(std::thread::spawn(move || {
                        let _ = writer.write_all(&data);
                    }));
                }
            }

            if let Some(mut stderr) = child.stderr.take() {
                stderr_readers.push(std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = stderr.read_to_end(&mut bytes);
                    bytes
                }));
            }

            let stdout = child.stdout.take()
                .ok_or_else(|| anyhow::anyhow!("pipeline: stdout de etapa no disponible"))?;
            if index + 1 == sources.len() {
                final_stdout_reader = Some(std::thread::spawn(move || {
                    let mut stdout = stdout;
                    let mut bytes = Vec::new();
                    let _ = stdout.read_to_end(&mut bytes);
                    bytes
                }));
            } else {
                previous_stdout = Some(stdout);
            }

            children.push(child);
        }

        let mut statuses = Vec::with_capacity(children.len());
        let mut aborted = false;
        for index in 0..children.len() {
            loop {
                if self.force_abort.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    for process in &mut children {
                        let _ = process.kill();
                    }
                    statuses.resize(sources.len(), 130);
                    aborted = true;
                    break;
                }

                if let Some(status) = children[index].try_wait()? {
                    statuses.push(status.code().unwrap_or(1));
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if aborted {
                break;
            }
        }

        if let Some(writer) = stdin_writer {
            let _ = writer.join();
        }

        let stdout = final_stdout_reader
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        let mut stderr = Vec::new();
        for reader in stderr_readers {
            if let Ok(mut bytes) = reader.join() {
                stderr.append(&mut bytes);
            }
        }

        if statuses.len() < sources.len() {
            statuses.resize(sources.len(), 130);
        }
        let status = statuses.last().copied().unwrap_or(0);
        Ok(Some((
            ExecutionResult::from_parts(
                String::from_utf8_lossy(&stdout).into_owned(),
                String::from_utf8_lossy(&stderr).into_owned(),
                status,
            ),
            statuses,
        )))
    }

    fn execute_external_background(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<u32> {
        let script = resolve_shell_script(program, cwd);
        let mut command = if let Some(script) = &script {
            let mut command = Command::new(std::env::current_exe()?);
            command.arg(script);
            command
        } else {
            Command::new(program)
        };
        command
            .args(args)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = command.spawn()
            .with_context(|| format!("no se pudo ejecutar {program} en background"))?;
        let pid = child.id();
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(
            pid,
            BackgroundJob { command: format!("{} {}", program, args.join(" ")).trim().to_owned(), child },
        );
        Ok(pid)
    }

    fn execute_shell_background(
        &self,
        source: &str,
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<u32> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("-c")
            .arg(source)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn().context("no se pudo lanzar job Bash en background")?;
        let pid = child.id();
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(
            pid,
            BackgroundJob { command: source.to_owned(), child },
        );
        Ok(pid)
    }

    fn execute_coproc(
        &self,
        source: &str,
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<CoprocHandles> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("-c")
            .arg(source)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().context("no se pudo lanzar coproc Bash")?;
        let pid = child.id();
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("coproc: no se pudo abrir stdout"))?;
        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("coproc: no se pudo abrir stdin"))?;

        self.jobs.lock().unwrap_or_else(|error| error.into_inner()).insert(
            pid,
            BackgroundJob { command: source.to_owned(), child },
        );

        Ok(CoprocHandles {
            pid,
            stdout: Box::new(stdout),
            stdin: Box::new(stdin),
        })
    }

    fn jobs(&self) -> Result<Vec<JobInfo>> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows = Vec::new();
        for (pid, job) in jobs.iter_mut() {
            let running = job.child.try_wait()?.is_none();
            rows.push(JobInfo {
                pid: *pid,
                command: job.command.clone(),
                running,
            });
        }
        rows.sort_by_key(|job| job.pid);
        Ok(rows)
    }

    fn wait_job(&self, pid: Option<u32>) -> Result<i32> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pid) = pid {
            let Some(mut job) = jobs.remove(&pid) else { return Ok(127); };
            return Ok(job.child.wait()?.code().unwrap_or(1));
        }

        let pids: Vec<u32> = jobs.keys().copied().collect();
        let mut status = 0;
        for pid in pids {
            if let Some(mut job) = jobs.remove(&pid) {
                status = job.child.wait()?.code().unwrap_or(1);
            }
        }
        Ok(status)
    }

    fn wait_next_job(&self) -> Result<Option<(u32, i32)>> {
        loop {
            let completed = {
                let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
                if jobs.is_empty() {
                    return Ok(None);
                }

                let mut completed = None;
                for (pid, job) in jobs.iter_mut() {
                    if let Some(status) = job.child.try_wait()? {
                        completed = Some((*pid, status.code().unwrap_or(1)));
                        break;
                    }
                }

                if let Some((pid, status)) = completed {
                    jobs.remove(&pid);
                    Some((pid, status))
                } else {
                    None
                }
            };

            if completed.is_some() {
                return Ok(completed);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn disown_job(&self, pid: u32) -> Result<bool> {
        Ok(self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pid)
            .is_some())
    }

    fn shell_times(&self) -> Result<(
        std::time::Duration,
        std::time::Duration,
        std::time::Duration,
        std::time::Duration,
    )> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::{
                Foundation::FILETIME,
                System::Threading::{GetCurrentProcess, GetProcessTimes},
            };

            fn duration(value: FILETIME) -> std::time::Duration {
                let ticks = ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64;
                std::time::Duration::from_nanos(ticks.saturating_mul(100))
            }

            let mut creation: FILETIME = std::mem::zeroed();
            let mut exit: FILETIME = std::mem::zeroed();
            let mut kernel: FILETIME = std::mem::zeroed();
            let mut user: FILETIME = std::mem::zeroed();

            if GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }

            return Ok((
                duration(user),
                duration(kernel),
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
            ));
        }

        #[cfg(not(windows))]
        {
            Ok((
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
                std::time::Duration::ZERO,
            ))
        }
    }
}

fn resolve_shell_script(program: &str, cwd: &Path) -> Option<PathBuf> {
    let candidate = {
        let path = PathBuf::from(program);
        if path.is_absolute() { path } else { cwd.join(path) }
    };
    if !candidate.is_file() { return None; }

    if candidate.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sh")) {
        return Some(candidate);
    }

    let prefix = std::fs::read(&candidate).ok()?;
    let first = prefix.split(|b| *b == b'\n').next().unwrap_or(&[]);
    let shebang = String::from_utf8_lossy(first).to_ascii_lowercase();
    if shebang.starts_with("#!") && (shebang.contains("bash") || shebang.contains("/sh")) {
        Some(candidate)
    } else {
        None
    }
}

pub struct NativeShellEngine {
    interpreter: Interpreter,
    config_file: PathBuf,
    interrupt: Arc<std::sync::atomic::AtomicBool>,
    force_abort: Arc<std::sync::atomic::AtomicBool>,
}

impl NativeShellEngine {
    pub fn new(registry: Arc<CommandRegistry>, config_file: PathBuf) -> Result<Self> {
        let interrupt = crate::adapters::terminal::io::interrupt_flag();
        let force_abort = crate::adapters::terminal::io::force_abort_flag();
        let host = WindowsShellHost {
            registry,
            interrupt: interrupt.clone(),
            force_abort: force_abort.clone(),
            jobs: Mutex::new(HashMap::new()),
        };
        let mut interpreter = Interpreter::new(Box::new(host));
        interpreter.env.export(
            "ADM_CONFIG",
            config_file.to_string_lossy().into_owned(),
        );
        if let Some(root) = config_file.parent().and_then(Path::parent) {
            interpreter.env.set(
                "HISTFILE",
                root.join("data").join("history").to_string_lossy().into_owned(),
            );
        }

        let mut engine = Self {
            interpreter,
            config_file,
            interrupt,
            force_abort,
        };
        engine.load_config()?;
        Ok(engine)
    }

    fn load_config(&mut self) -> Result<()> {
        if self.config_file.exists() {
            let source = std::fs::read_to_string(&self.config_file)?;
            let result = self.interpreter.execute_text(&source)?;
            if !result.stderr.is_empty() {
                eprint!("{}", result.stderr);
            }
        }
        Ok(())
    }
}

impl ShellEngine for NativeShellEngine {
    fn set_arguments(&mut self, name: &str, args: &[String]) {
        self.interpreter.env.script_name = name.to_owned();
        self.interpreter.env.positional = args.to_vec();
    }
    fn working_dir(&self) -> &Path {
        &self.interpreter.env.cwd
    }

    fn set_interactive(&mut self, interactive: bool) {
        self.interpreter.set_interactive(interactive);
    }

    fn complete(&mut self, line: &str, cursor: usize) -> Result<Vec<String>> {
        self.interpreter.complete_line(line, cursor)
    }

    fn prepare_history(&mut self, line: &str) -> Result<(String, bool)> {
        self.interpreter.prepare_history_line(line)
    }

    fn record_history(&mut self, line: &str) -> Result<()> {
        self.interpreter.record_history_line(line)
    }

    fn readline_bindings(&self) -> HashMap<String, String> {
        self.interpreter.readline_bindings()
    }

    fn run_readline_binding(
        &mut self,
        command: &str,
        line: &str,
        cursor: usize,
    ) -> Result<(String, usize, String, String)> {
        let (line, cursor, result) = self.interpreter.run_readline_shell_binding(command, line, cursor)?;
        Ok((line, cursor, result.stdout, result.stderr))
    }

    fn execute(&mut self, line: &str) -> Result<ShellExecution> {
        self.interrupt.store(false, std::sync::atomic::Ordering::SeqCst);
        self.force_abort.store(false, std::sync::atomic::Ordering::SeqCst);
        let result = self.interpreter.execute_text(line)?;

        let mut execution = if result.exit_requested {
            ShellExecution::exit(result.status)
        } else {
            ShellExecution::continue_running(result.status)
        };
        execution.stdout = result.stdout;
        execution.stderr = result.stderr;
        Ok(execution)
    }
}
