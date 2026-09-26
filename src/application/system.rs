use std::{
    env,
    io::{Write, stdout},
    thread,
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use sysinfo::{Disks, Pid, System};

use crate::{adapters::terminal::guard::AlternateScreenGuard, core::CommandOutput};

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("info");

    match sub {
        "info" => info(),
        "processes" | "ps" => processes(),
        "top" => top(),
        "disks" | "df" => disks(),
        "memory" | "free" => memory(),
        "hostname" => hostname(),
        "whoami" => whoami(),
        "uname" => uname(&args[1..]),
        "kill" => kill_process(&args[1..]),
        "services" => Ok(CommandOutput::error(
            "sys services: usa 'sc query' por ahora; proveedor Rust pendiente",
            2,
        )),
        _ => Ok(CommandOutput::error(
            format!("sys: subcomando desconocido: {sub}"),
            2,
        )),
    }
}

pub fn run_alias(name: &str, args: &[String]) -> anyhow::Result<CommandOutput> {
    match name {
        "ps" => processes(),
        "top" => top(),
        "df" => disks(),
        "free" => memory(),
        "hostname" => hostname(),
        "whoami" => whoami(),
        "uname" => uname(args),
        "kill" => kill_process(args),
        _ => Ok(CommandOutput::error(
            format!("comando de sistema desconocido: {name}"),
            127,
        )),
    }
}

fn info() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();

    Ok(CommandOutput::ok(format!(
        "hostname: {}\nos: {}\nkernel: {}\nuptime: {} s\ncpus: {}\nmemory_total: {} MiB\nmemory_used: {} MiB\n",
        System::host_name().unwrap_or_else(|| "desconocido".to_owned()),
        System::long_os_version().unwrap_or_else(|| "Windows".to_owned()),
        System::kernel_version().unwrap_or_else(|| "desconocido".to_owned()),
        System::uptime(),
        system.cpus().len(),
        system.total_memory() / 1024 / 1024,
        system.used_memory() / 1024 / 1024,
    )))
}

fn processes() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();

    let mut rows: Vec<_> = system.processes().iter().collect();
    rows.sort_by(|a, b| {
        b.1.cpu_usage()
            .partial_cmp(&a.1.cpu_usage())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = String::from("PID      CPU%     RAM MiB   PROCESS\n");

    for (pid, process) in rows.into_iter().take(80) {
        out.push_str(&format!(
            "{:<8} {:>6.1} {:>10.1}   {}\n",
            pid,
            process.cpu_usage(),
            process.memory() as f64 / 1024.0 / 1024.0,
            process.name().to_string_lossy()
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn disks() -> anyhow::Result<CommandOutput> {
    let disks = Disks::new_with_refreshed_list();
    let mut out = String::from("MOUNT                 TOTAL GiB   USED GiB   FREE GiB   USE%   FS\n");

    for disk in disks.list() {
        let total = disk.total_space();
        let free = disk.available_space();
        let used = total.saturating_sub(free);
        let percent = if total == 0 {
            0.0
        } else {
            used as f64 * 100.0 / total as f64
        };

        out.push_str(&format!(
            "{:<20} {:>9.1} {:>9.1} {:>10.1} {:>5.1}%   {}\n",
            disk.mount_point().display(),
            total as f64 / 1024.0 / 1024.0 / 1024.0,
            used as f64 / 1024.0 / 1024.0 / 1024.0,
            free as f64 / 1024.0 / 1024.0 / 1024.0,
            percent,
            disk.file_system().to_string_lossy()
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn memory() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_memory();

    Ok(CommandOutput::ok(format!(
        "              total        used        free\nMem:      {:>10}  {:>10}  {:>10} MiB\nSwap:     {:>10}  {:>10}  {:>10} MiB\n",
        system.total_memory() / 1024 / 1024,
        system.used_memory() / 1024 / 1024,
        system.available_memory() / 1024 / 1024,
        system.total_swap() / 1024 / 1024,
        system.used_swap() / 1024 / 1024,
        system.total_swap().saturating_sub(system.used_swap()) / 1024 / 1024,
    )))
}

fn hostname() -> anyhow::Result<CommandOutput> {
    Ok(CommandOutput::ok(format!(
        "{}\n",
        System::host_name().unwrap_or_else(|| "desconocido".to_owned())
    )))
}

fn whoami() -> anyhow::Result<CommandOutput> {
    let user = env::var("USERNAME").unwrap_or_else(|_| "desconocido".to_owned());
    let domain = env::var("USERDOMAIN").unwrap_or_default();

    if domain.is_empty() || domain.eq_ignore_ascii_case(&user) {
        Ok(CommandOutput::ok(format!("{user}\n")))
    } else {
        Ok(CommandOutput::ok(format!(
            "{}\\{}\n",
            domain.to_ascii_lowercase(),
            user.to_ascii_lowercase()
        )))
    }
}

fn uname(args: &[String]) -> anyhow::Result<CommandOutput> {
    let all = args.iter().any(|arg| arg == "-a");
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_owned());
    let host = System::host_name().unwrap_or_else(|| "unknown".to_owned());
    let os = System::name().unwrap_or_else(|| "Windows".to_owned());
    let arch = env::consts::ARCH;

    if all {
        Ok(CommandOutput::ok(format!(
            "ADM-Windows {host} {kernel} {arch} {os}\n"
        )))
    } else {
        Ok(CommandOutput::ok("ADM-Windows\n"))
    }
}

fn kill_process(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(pid_text) = args.first() else {
        return Ok(CommandOutput::error("kill: uso: kill PID", 2));
    };

    let pid_value = pid_text
        .trim_start_matches('-')
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("kill: PID inválido: {pid_text}"))?;

    let system = System::new_all();
    let pid = Pid::from_u32(pid_value);

    let Some(process) = system.process(pid) else {
        return Ok(CommandOutput::error(
            format!("kill: no existe el proceso {pid_value}"),
            1,
        ));
    };

    if process.kill() {
        Ok(CommandOutput::ok(""))
    } else {
        Ok(CommandOutput::error(
            format!("kill: no se pudo terminar {pid_value}"),
            1,
        ))
    }
}

fn top() -> anyhow::Result<CommandOutput> {
    let _guard = AlternateScreenGuard::enter()?;
    let mut sort_cpu = true;

    loop {
        let mut system = System::new_all();
        system.refresh_all();

        let mut rows: Vec<_> = system.processes().iter().collect();

        if sort_cpu {
            rows.sort_by(|a, b| {
                b.1.cpu_usage()
                    .partial_cmp(&a.1.cpu_usage())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        } else {
            rows.sort_by_key(|(_, process)| std::cmp::Reverse(process.memory()));
        }

        let (width, height) = terminal::size()?;
        let max_rows = height.saturating_sub(6) as usize;

        execute!(
            stdout(),
            cursor::MoveTo(0, 0),
            Clear(ClearType::All)
        )?;

        println!(
            "ADM top  uptime {}s  CPU {} cores  Mem {} / {} MiB",
            System::uptime(),
            system.cpus().len(),
            system.used_memory() / 1024 / 1024,
            system.total_memory() / 1024 / 1024
        );
        println!(
            "orden: {}   [c] CPU  [m] memoria  [q] salir",
            if sort_cpu { "CPU" } else { "MEM" }
        );
        println!();
        println!("PID      CPU%     RAM MiB   PROCESS");

        for (pid, process) in rows.into_iter().take(max_rows) {
            let mut name = process.name().to_string_lossy().into_owned();
            let name_width = width.saturating_sub(32) as usize;
            if name.len() > name_width && name_width > 3 {
                name.truncate(name_width.saturating_sub(3));
                name.push_str("...");
            }

            println!(
                "{:<8} {:>6.1} {:>10.1}   {}",
                pid,
                process.cpu_usage(),
                process.memory() as f64 / 1024.0 / 1024.0,
                name
            );
        }

        stdout().flush()?;

        if event::poll(Duration::from_millis(900))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') => sort_cpu = true,
                    KeyCode::Char('m') => sort_cpu = false,
                    _ => {}
                }
            }
        } else {
            thread::sleep(Duration::from_millis(100));
        }
    }

    Ok(CommandOutput::ok(""))
}
