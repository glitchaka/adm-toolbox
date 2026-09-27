use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicI32, Ordering},
        mpsc,
        Arc, Mutex,
    },
};

use anyhow::{Context, Result};

#[cfg(windows)]
use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, GetFileInformationByHandleEx, GetFileType, ReadFile,
        WriteFile, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_CHAR, FILE_TYPE_PIPE,
        FileAttributeTagInfo, OPEN_EXISTING, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND,
    },
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        },
        Threading::{GetCurrentProcess, GetProcessTimes},
    },
};

use crate::{
    builtins::CommandRegistry,
    core::{
        CommandContext,
        ShellExecution,
        bash::{ExecutionResult, Interpreter, JobInfo, ShellCommandHost},
        ports::ShellEngine,
    },
};

struct BackgroundJob {
    command: String,
    child: Child,
    pipe_fds: Vec<i32>,
}

struct VirtualReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    eof: bool,
}

enum VirtualPipe {
    Reader(VirtualReader),
    Writer(ChildStdin),
}

struct WindowsShellHost {
    registry: Arc<CommandRegistry>,
    interrupt: Arc<std::sync::atomic::AtomicBool>,
    force_abort: Arc<std::sync::atomic::AtomicBool>,
    jobs: Mutex<HashMap<u32, BackgroundJob>>,
    pipes: Mutex<HashMap<i32, VirtualPipe>>,
    next_fd: AtomicI32,
    child_cpu_100ns: Mutex<(u64, u64)>,
}

impl WindowsShellHost {
    #[cfg(windows)]
    fn process_cpu_100ns(handle: HANDLE) -> Option<(u64, u64)> {
        unsafe {
            let mut creation: FILETIME = std::mem::zeroed();
            let mut exit: FILETIME = std::mem::zeroed();
            let mut kernel: FILETIME = std::mem::zeroed();
            let mut user: FILETIME = std::mem::zeroed();
            if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
                return None;
            }
            let to_u64 = |time: FILETIME| {
                (time.dwLowDateTime as u64) | ((time.dwHighDateTime as u64) << 32)
            };
            Some((to_u64(user), to_u64(kernel)))
        }
    }

    #[cfg(not(windows))]
    fn process_cpu_100ns(_handle: *mut std::ffi::c_void) -> Option<(u64, u64)> {
        None
    }

    fn record_child_cpu(&self, child: &Child) {
        #[cfg(windows)]
        {
            let handle = child.as_raw_handle() as HANDLE;
            if let Some((user, kernel)) = Self::process_cpu_100ns(handle) {
                let mut total = self.child_cpu_100ns.lock().unwrap_or_else(|error| error.into_inner());
                total.0 = total.0.saturating_add(user);
                total.1 = total.1.saturating_add(kernel);
            }
        }
    }
}

impl ShellCommandHost for WindowsShellHost {
    fn interrupted(&self) -> bool { self.interrupt.load(std::sync::atomic::Ordering::SeqCst) }

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
    fn read_input(
        &self,
        prompt: &str,
        silent: bool,
        delimiter: char,
        max_chars: Option<usize>,
        timeout: Option<std::time::Duration>,
        initial: &str,
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
        let started = std::time::Instant::now();

        loop {
            if let Some(limit) = max_chars {
                if line.chars().count() >= limit {
                    crate::adapters::terminal::io::write(b"\r\n")?;
                    return Ok(Some(line));
                }
            }

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
                    KeyCode::Enter if delimiter == '\n' => {
                        crate::adapters::terminal::io::write(b"\r\n")?;
                        return Ok(Some(line));
                    }
                    KeyCode::Backspace => {
                        if line.pop().is_some() && !silent {
                            crate::adapters::terminal::io::write(b"\x08 \x08")?;
                        }
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if ch == delimiter {
                            if !silent && delimiter != '\n' {
                                let mut buf = [0u8; 4];
                                crate::adapters::terminal::io::write(ch.encode_utf8(&mut buf).as_bytes())?;
                            }
                            return Ok(Some(line));
                        }
                        line.push(ch);
                        if !silent {
                            let mut buf = [0u8; 4];
                            crate::adapters::terminal::io::write(ch.encode_utf8(&mut buf).as_bytes())?;
                        }
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    for ch in text.chars() {
                        if ch == delimiter {
                            return Ok(Some(line));
                        }
                        line.push(ch);
                        if max_chars.is_some_and(|limit| line.chars().count() >= limit) {
                            break;
                        }
                    }
                    if !silent { crate::adapters::terminal::io::write(text.as_bytes())?; }
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

    fn create_process_substitution_pipe(
        &self,
        direction: char,
        source: &str,
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<Option<PathBuf>> {
        #[cfg(windows)]
        {
            let id = self.next_fd.fetch_add(1, Ordering::SeqCst);
            let pipe_name = format!(
                r"\\.\pipe\shell-shock-ps-{}-{}",
                std::process::id(),
                id
            );
            let wide: Vec<u16> = std::ffi::OsStr::new(&pipe_name)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let access = if direction == '<' {
                PIPE_ACCESS_OUTBOUND
            } else {
                PIPE_ACCESS_INBOUND
            };
            let handle = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    access,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    65_536,
                    65_536,
                    0,
                    std::ptr::null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error().into());
            }

            let handle_value = handle as isize;
            let source = source.to_owned();
            let cwd = cwd.to_path_buf();
            let env = env.clone();
            let display = crate::adapters::terminal::io::output_sender();

            std::thread::spawn(move || {
                let handle = handle_value as HANDLE;
                let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
                if connected == 0 {
                    // ERROR_PIPE_CONNECTED means the client won the race between
                    // CreateNamedPipe and ConnectNamedPipe and is already usable.
                    let error = unsafe { GetLastError() };
                    if error != 535 {
                        unsafe { CloseHandle(handle); }
                        return;
                    }
                }

                if direction == '<' {
                    let mut command = match std::env::current_exe() {
                        Ok(exe) => Command::new(exe),
                        Err(_) => {
                            unsafe {
                                DisconnectNamedPipe(handle);
                                CloseHandle(handle);
                            }
                            return;
                        }
                    };
                    command.arg("-c")
                        .arg(&source)
                        .current_dir(&cwd)
                        .envs(&env)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());

                    if let Ok(mut child) = command.spawn() {
                        let stderr = child.stderr.take();
                        let display_stderr = display.clone();
                        let stderr_thread = stderr.map(|mut stderr| std::thread::spawn(move || {
                            let mut bytes = Vec::new();
                            let _ = stderr.read_to_end(&mut bytes);
                            send_async_terminal_output(display_stderr, &bytes, true);
                        }));

                        if let Some(mut stdout) = child.stdout.take() {
                            let mut buffer = [0u8; 8192];
                            loop {
                                match stdout.read(&mut buffer) {
                                    Ok(0) | Err(_) => break,
                                    Ok(size) => {
                                        let mut offset = 0usize;
                                        while offset < size {
                                            let mut written = 0u32;
                                            let ok = unsafe {
                                                WriteFile(
                                                    handle,
                                                    buffer[offset..size].as_ptr(),
                                                    (size - offset) as u32,
                                                    &mut written,
                                                    std::ptr::null_mut(),
                                                )
                                            };
                                            if ok == 0 || written == 0 { break; }
                                            offset += written as usize;
                                        }
                                    }
                                }
                            }
                        }
                        let _ = child.wait();
                        if let Some(thread) = stderr_thread { let _ = thread.join(); }
                    }
                } else {
                    let mut command = match std::env::current_exe() {
                        Ok(exe) => Command::new(exe),
                        Err(_) => {
                            unsafe {
                                DisconnectNamedPipe(handle);
                                CloseHandle(handle);
                            }
                            return;
                        }
                    };
                    command.arg("-c")
                        .arg(&source)
                        .current_dir(&cwd)
                        .envs(&env)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());

                    if let Ok(mut child) = command.spawn() {
                        if let Some(mut stdin) = child.stdin.take() {
                            let mut buffer = [0u8; 8192];
                            loop {
                                let mut read = 0u32;
                                let ok = unsafe {
                                    ReadFile(
                                        handle,
                                        buffer.as_mut_ptr(),
                                        buffer.len() as u32,
                                        &mut read,
                                        std::ptr::null_mut(),
                                    )
                                };
                                if ok == 0 || read == 0 { break; }
                                if stdin.write_all(&buffer[..read as usize]).is_err() { break; }
                            }
                        }
                        if let Ok(output) = child.wait_with_output() {
                            send_async_terminal_output(display.clone(), &output.stdout, false);
                            send_async_terminal_output(display.clone(), &output.stderr, true);
                        }
                    }
                }

                unsafe {
                    FlushFileBuffers(handle);
                    DisconnectNamedPipe(handle);
                    CloseHandle(handle);
                }
            });

            return Ok(Some(PathBuf::from(pipe_name)));
        }
        #[cfg(not(windows))]
        {
            let _ = (direction, source, cwd, env);
            Ok(None)
        }
    }

    fn process_times(&self) -> Result<(f64, f64, f64, f64)> {
        #[cfg(windows)]
        {
            let (user, kernel) = Self::process_cpu_100ns(unsafe { GetCurrentProcess() })
                .unwrap_or((0, 0));
            let child = *self.child_cpu_100ns.lock().unwrap_or_else(|error| error.into_inner());
            let seconds = |ticks: u64| ticks as f64 / 10_000_000.0;
            return Ok((seconds(user), seconds(kernel), seconds(child.0), seconds(child.1)));
        }
        #[cfg(not(windows))]
        {
            Ok((0.0, 0.0, 0.0, 0.0))
        }
    }

    fn file_type_test(&self, path: &Path, kind: char) -> Result<Option<bool>> {
        #[cfg(windows)]
        {
            let raw = path.to_string_lossy().replace('/', "\\");
            let lower = raw.to_ascii_lowercase();

            if kind == 'b' {
                return Ok(Some(
                    lower.starts_with(r"\\.\physicaldrive")
                        || lower.starts_with(r"\\?\volume{")
                        || lower.starts_with(r"\\.\harddisk"),
                ));
            }
            if kind == 'p' && lower.starts_with(r"\\.\pipe\") {
                return Ok(Some(true));
            }

            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    FILE_READ_ATTRIBUTES,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Ok(Some(false));
            }

            let file_type = unsafe { GetFileType(handle) };
            let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
            let tag_ok = unsafe {
                GetFileInformationByHandleEx(
                    handle,
                    FileAttributeTagInfo,
                    (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                    std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
                )
            } != 0;
            unsafe { CloseHandle(handle); }

            const IO_REPARSE_TAG_AF_UNIX: u32 = 0x80000023;
            let value = match kind {
                'c' => file_type == FILE_TYPE_CHAR,
                'p' => file_type == FILE_TYPE_PIPE && lower.starts_with(r"\\.\pipe\"),
                'S' => tag_ok && tag.ReparseTag == IO_REPARSE_TAG_AF_UNIX,
                _ => false,
            };
            return Ok(Some(value));
        }
        #[cfg(not(windows))]
        {
            let _ = (path, kind);
            Ok(None)
        }
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
            if self.force_abort.swap(false, Ordering::SeqCst)
                || self.interrupt.swap(false, Ordering::SeqCst)
            {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(ExecutionResult::from_parts(
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                    130,
                ));
            }

            if child.try_wait()?.is_some() {
                self.record_child_cpu(&child);
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
        commands: &[String],
        stderr_to_pipe: &[bool],
        cwd: &Path,
        env: &HashMap<String, String>,
        stdin: Option<&[u8]>,
    ) -> Result<Option<(ExecutionResult, Vec<i32>)>> {
        if commands.is_empty() {
            return Ok(Some((ExecutionResult::success(), Vec::new())));
        }

        let exe = std::env::current_exe()?;
        let mut children = Vec::with_capacity(commands.len());
        let mut stderr_readers = Vec::with_capacity(commands.len());
        let mut previous_stdout = None;
        let mut input_writer = None;
        let mut final_stdout = None;

        for (index, source) in commands.iter().enumerate() {
            let pipe_stderr = stderr_to_pipe.get(index).copied().unwrap_or(false)
                && index + 1 < commands.len();
            let source = if pipe_stderr {
                format!("{{ {source}; }} 2>&1")
            } else {
                source.clone()
            };

            let mut command = Command::new(&exe);
            command
                .arg("-c")
                .arg(source)
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
                    .ok_or_else(|| anyhow::anyhow!("pipeline: stdout anterior no disponible"))?;
                command.stdin(Stdio::from(upstream));
            }

            let mut child = command.spawn()
                .with_context(|| format!("no se pudo lanzar etapa {} del pipeline", index + 1))?;

            if index == 0 {
                if let (Some(bytes), Some(mut writer)) = (stdin, child.stdin.take()) {
                    let bytes = bytes.to_vec();
                    input_writer = Some(std::thread::spawn(move || {
                        let _ = writer.write_all(&bytes);
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

            if index + 1 < commands.len() {
                previous_stdout = child.stdout.take();
            } else if let Some(mut stdout) = child.stdout.take() {
                final_stdout = Some(std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = stdout.read_to_end(&mut bytes);
                    bytes
                }));
            }

            children.push(child);
        }

        let mut statuses = vec![None; children.len()];
        while statuses.iter().any(Option::is_none) {
            if self.force_abort.swap(false, Ordering::SeqCst)
                || self.interrupt.swap(false, Ordering::SeqCst)
            {
                for child in &mut children {
                    let _ = child.kill();
                }
            }

            for (index, child) in children.iter_mut().enumerate() {
                if statuses[index].is_none() {
                    if let Some(status) = child.try_wait()? {
                        self.record_child_cpu(child);
                        statuses[index] = Some(status.code().unwrap_or(1));
                    }
                }
            }

            if statuses.iter().any(Option::is_none) {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        if let Some(writer) = input_writer {
            let _ = writer.join();
        }

        let stdout = final_stdout
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();

        let mut stderr = Vec::new();
        for reader in stderr_readers {
            if let Ok(mut bytes) = reader.join() {
                stderr.append(&mut bytes);
            }
        }

        let statuses: Vec<i32> = statuses.into_iter().map(|status| status.unwrap_or(1)).collect();
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

    fn start_coproc(
        &self,
        source: &str,
        cwd: &Path,
        env: &HashMap<String, String>,
    ) -> Result<Option<(u32, i32, i32)>> {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("-c")
            .arg(source)
            .current_dir(cwd)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().context("no se pudo iniciar coproc Bash")?;
        let stdin = child.stdin.take().context("coproc sin stdin")?;
        let mut stdout = child.stdout.take().context("coproc sin stdout")?;
        let pid = child.id();

        let read_fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        let write_fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();

        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(size) => {
                        if sender.send(chunk[..size].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        {
            let mut pipes = self.pipes.lock().unwrap_or_else(|error| error.into_inner());
            pipes.insert(read_fd, VirtualPipe::Reader(VirtualReader {
                receiver,
                buffer: Vec::new(),
                eof: false,
            }));
            pipes.insert(write_fd, VirtualPipe::Writer(stdin));
        }

        self.jobs.lock().unwrap_or_else(|error| error.into_inner()).insert(
            pid,
            BackgroundJob {
                command: source.to_owned(),
                child,
                pipe_fds: vec![read_fd, write_fd],
            },
        );

        Ok(Some((pid, read_fd, write_fd)))
    }

    fn read_fd(
        &self,
        fd: i32,
        delimiter: char,
        max_chars: Option<usize>,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<String>> {
        let started = std::time::Instant::now();
        let delimiter_bytes = delimiter.to_string().into_bytes();
        let mut pipes = self.pipes.lock().unwrap_or_else(|error| error.into_inner());
        let Some(VirtualPipe::Reader(reader)) = pipes.get_mut(&fd) else {
            return Ok(None);
        };

        loop {
            if let Some(position) = find_byte_sequence(&reader.buffer, &delimiter_bytes) {
                let bytes = reader.buffer
                    .drain(..position + delimiter_bytes.len())
                    .collect::<Vec<_>>();
                let content = &bytes[..bytes.len().saturating_sub(delimiter_bytes.len())];
                return Ok(Some(limit_utf8_chars(content, max_chars)));
            }

            if let Some(limit) = max_chars {
                if String::from_utf8_lossy(&reader.buffer).chars().count() >= limit {
                    return Ok(Some(take_utf8_chars(&mut reader.buffer, limit)));
                }
            }

            if reader.eof {
                if reader.buffer.is_empty() {
                    return Ok(None);
                }
                let bytes = std::mem::take(&mut reader.buffer);
                return Ok(Some(limit_utf8_chars(&bytes, max_chars)));
            }

            let next = if let Some(limit) = timeout {
                let elapsed = started.elapsed();
                if elapsed >= limit {
                    return Ok(None);
                }
                match reader.receiver.recv_timeout(limit - elapsed) {
                    Ok(bytes) => Some(bytes),
                    Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                    Err(mpsc::RecvTimeoutError::Disconnected) => None,
                }
            } else {
                reader.receiver.recv().ok()
            };

            match next {
                Some(bytes) => reader.buffer.extend_from_slice(&bytes),
                None => reader.eof = true,
            }
        }
    }

    fn write_fd(&self, fd: i32, data: &[u8]) -> Result<bool> {
        let mut pipes = self.pipes.lock().unwrap_or_else(|error| error.into_inner());
        let Some(VirtualPipe::Writer(writer)) = pipes.get_mut(&fd) else {
            return Ok(false);
        };
        writer.write_all(data)?;
        writer.flush()?;
        Ok(true)
    }

    fn close_fd(&self, fd: i32) -> Result<bool> {
        Ok(self.pipes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&fd)
            .is_some())
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
            BackgroundJob {
                command: format!("{} {}", program, args.join(" ")).trim().to_owned(),
                child,
                pipe_fds: Vec::new(),
            },
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
            BackgroundJob { command: source.to_owned(), child, pipe_fds: Vec::new() },
        );
        Ok(pid)
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
        if let Some(pid) = pid {
            let job = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(&pid);
            let Some(mut job) = job else { return Ok(127); };
            let status = job.child.wait()?.code().unwrap_or(1);
            self.record_child_cpu(&job.child);
            for fd in job.pipe_fds {
                let _ = self.close_fd(fd)?;
            }
            return Ok(status);
        }

        let pids: Vec<u32> = self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        let mut status = 0;
        for pid in pids {
            let job = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(&pid);
            if let Some(mut job) = job {
                status = job.child.wait()?.code().unwrap_or(1);
                self.record_child_cpu(&job.child);
                for fd in job.pipe_fds {
                    let _ = self.close_fd(fd)?;
                }
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
                        self.record_child_cpu(&job.child);
                        completed = Some((*pid, status.code().unwrap_or(1)));
                        break;
                    }
                }

                if let Some((pid, status)) = completed {
                    let pipe_fds = jobs.remove(&pid).map(|job| job.pipe_fds).unwrap_or_default();
                    Some((pid, status, pipe_fds))
                } else {
                    None
                }
            };

            if let Some((pid, status, pipe_fds)) = completed {
                for fd in pipe_fds {
                    let _ = self.close_fd(fd)?;
                }
                return Ok(Some((pid, status)));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn disown_job(&self, pid: u32) -> Result<bool> {
        let job = self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pid);
        if let Some(job) = job {
            for fd in job.pipe_fds {
                let _ = self.close_fd(fd)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn send_async_terminal_output(
    sender: Option<mpsc::Sender<Vec<u8>>>,
    bytes: &[u8],
    stderr: bool,
) {
    if bytes.is_empty() { return; }
    if let Some(sender) = sender {
        let mut translated = Vec::with_capacity(bytes.len());
        for (index, byte) in bytes.iter().copied().enumerate() {
            if byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
                translated.push(b'\r');
            }
            translated.push(byte);
        }
        let _ = sender.send(translated);
    } else if stderr {
        let _ = std::io::stderr().write_all(bytes);
        let _ = std::io::stderr().flush();
    } else {
        let _ = std::io::stdout().write_all(bytes);
        let _ = std::io::stdout().flush();
    }
}

fn find_byte_sequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn limit_utf8_chars(bytes: &[u8], limit: Option<usize>) -> String {
    let text = String::from_utf8_lossy(bytes);
    match limit {
        Some(limit) => text.chars().take(limit).collect(),
        None => text.into_owned(),
    }
}

fn take_utf8_chars(buffer: &mut Vec<u8>, count: usize) -> String {
    let text = String::from_utf8_lossy(buffer);
    let end = text.char_indices()
        .nth(count)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    let bytes = buffer.drain(..end).collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).into_owned()
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
            pipes: Mutex::new(HashMap::new()),
            next_fd: AtomicI32::new(10),
            child_cpu_100ns: Mutex::new((0, 0)),
        };
        let mut interpreter = Interpreter::new(Box::new(host));
        interpreter.env.export(
            "ADM_CONFIG",
            config_file.to_string_lossy().into_owned(),
        );
        let history_file = config_file.parent()
            .and_then(|config_dir| config_dir.parent())
            .map(|root| root.join("data").join("history"))
            .unwrap_or_else(|| PathBuf::from("data").join("history"));
        interpreter.env.set("HISTFILE", history_file.to_string_lossy().into_owned());

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

    fn set_interactive(&mut self, interactive: bool) {
        self.interpreter.set_interactive(interactive);
    }

    fn prepare_prompt(&mut self, continuation: bool) -> Result<(String, String, Option<String>)> {
        self.interpreter.prepare_prompt(continuation)
    }

    fn pre_execute_prompt(&mut self) -> Result<String> {
        self.interpreter.pre_execute_prompt()
    }

    fn input_timeout(&self) -> Option<std::time::Duration> {
        self.interpreter.input_timeout()
    }

    fn complete(&mut self, line: &str, cursor: usize) -> Result<Vec<String>> {
        self.interpreter.complete_line(line, cursor)
    }

    fn prepare_history(&mut self, line: &str) -> Result<(String, bool)> {
        self.interpreter.prepare_history(line)
    }

    fn record_history(&mut self, line: &str) -> Result<()> {
        self.interpreter.record_history(line)
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
        self.interpreter.run_readline_binding(command, line, cursor)
    }

    fn working_dir(&self) -> &Path {
        &self.interpreter.env.cwd
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
