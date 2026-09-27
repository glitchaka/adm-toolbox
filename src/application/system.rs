use std::{
    env,
    fmt::Write as _,
    sync::Arc,
    time::Duration,
};

use sysinfo::{Disks, Pid, System};

use crate::core::{
    CommandOutput,
    ports::{TerminalFactory, TerminalKey},
};

pub struct SystemService {
    terminal: Arc<dyn TerminalFactory>,
}

impl SystemService {
    pub fn new(terminal: Arc<dyn TerminalFactory>) -> Self {
        Self { terminal }
    }

    pub fn execute(&self, args: &[String]) -> anyhow::Result<CommandOutput> {
        let sub = args.first().map(String::as_str).unwrap_or("info");

        match sub {
            "info" => info(),
            "processes" | "ps" => processes(),
            "top" => self.top(),
            "disks" | "df" => disks(),
            "memory" | "free" => memory(),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(&args[1..]),
            "kill" => kill_process(&args[1..]),
            "fetch" | "neofetch" | "fastfetch" => fetch(&args[1..]),
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

    pub fn execute_alias(&self, name: &str, args: &[String]) -> anyhow::Result<CommandOutput> {
        match name {
            "ps" => processes(),
            "top" => self.top(),
            "df" => disks(),
            "free" => memory(),
            "hostname" => hostname(),
            "whoami" => whoami(),
            "uname" => uname(args),
            "kill" => kill_process(args),
            "fetch" | "neofetch" | "fastfetch" => fetch(args),
            _ => Ok(CommandOutput::error(
                format!("comando de sistema desconocido: {name}"),
                127,
            )),
        }
    }

    fn top(&self) -> anyhow::Result<CommandOutput> {
        let mut terminal = self.terminal.alternate_screen()?;
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

            let (width, height) = terminal.size()?;
            let max_rows = height.saturating_sub(6) as usize;
            let mut screen = String::new();

            writeln!(
                screen,
                "ADM top  uptime {}s  CPU {} cores  Mem {} / {} MiB",
                System::uptime(),
                system.cpus().len(),
                system.used_memory() / 1024 / 1024,
                system.total_memory() / 1024 / 1024
            )?;
            writeln!(
                screen,
                "orden: {}   [c] CPU  [m] memoria  [q] salir",
                if sort_cpu { "CPU" } else { "MEM" }
            )?;
            writeln!(screen)?;
            writeln!(screen, "PID      CPU%     RAM MiB   PROCESS")?;

            for (pid, process) in rows.into_iter().take(max_rows) {
                let mut name = process.name().to_string_lossy().into_owned();
                let name_width = width.saturating_sub(32) as usize;

                if name.chars().count() > name_width && name_width > 3 {
                    name = name.chars().take(name_width - 3).collect();
                    name.push_str("...");
                }

                writeln!(
                    screen,
                    "{:<8} {:>6.1} {:>10.1}   {}",
                    pid,
                    process.cpu_usage(),
                    process.memory() as f64 / 1024.0 / 1024.0,
                    name
                )?;
            }

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;

            match terminal.poll_key(Duration::from_millis(900))? {
                Some(TerminalKey::Char('q') | TerminalKey::Escape) => break,
                Some(TerminalKey::Char('c')) => sort_cpu = true,
                Some(TerminalKey::Char('m')) => sort_cpu = false,
                _ => {}
            }
        }

        Ok(CommandOutput::ok(""))
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


const FETCH_MASCOT_FULL: &str = r#"
        ..,,::;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;::,,..
      ..;;11ttttttffffffffffffLLLLLLLLLLLLLLfftttttttttttttttttttttttttttttttttttt111111111111ii;;..
    ..;;11fffffffftttttttttttttt11111111111111111111111111111111111111111111111111ffLLiiCC11fftt11;;
    ,,11tttttt11111111111111111111111111111111111111111111111111111111111111111111ttffiiff1111111111,,
    ::1111111111ffffffLLLLfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffftt1111111111::
    ;;1111iiCC88@@@@@@@@@@@@8888888888888888888888888888888888888888888888888888888888888888CCii1111;;
    ;;1111LL88@@@@88@@@@@@88888888888888888888888888888888888888888888888888888888888888888888LL1111;;
    ;;11ii88888888@@@@88888888888888888888888888888888888888888888888888888888888888888888888888ii11;;
    ;;1111888888@@@@88888888888888888888888888888888888888888888888888888888888888888888888888881111;;
    ;;1111888888CC8888888888888888888888888888888888888888888888888888888888888888888888888888881111;;
    ;;ii11888888ttff8888888888888800GG888888888888888888888888888888GG0088888888888888888888888811ii;;
    ;;ii1188888800ttff8888888888ffiiii118888888888888888888888888811ii;;tt888888888888888888888811ii;;
    ;;iiii888888880011LL888888CCiiLL11;;ff8888888888888888888888ffiiCCii;;CC88888888888888888888iiii;;
    ;;iiii88888888ff1100888888LLtt@@GG;;tt8888888888888888888888ttLL@@LL;;LL88888888888888888888iiii;;
    ;;iiii888888ff110088888888LLiiCCtt;;tt8888888888888888888888ttiiCCii;;LL88888888888888888888iiii;;
    ;;iiii888800tt008888888888LL;;;;LLiitt8888888888888888888888tt;;;;CC;;LL88888888888888888888iiii;;
    ;;iiii88888888888888888888LL;;;;;;;;tt8888888888888888888888tt;;;;;;;;LL88888888888888888888iiii;;
    ;;iiii88888888888888888888LL;;;;;;;;tt8888888888888888888888tt;;;;;;;;LL88888888888888888888iiii;;
    ;;iiii88888888888888888888LL;;;;;;;;ff8888888888888888888888ff;;;;;;;;CC88888888888888888888iiii;;
    ;;iiii8888888888888888888800ii;;;;ii00888888888888888888888800ii;;;;110088888888888888888888iiii;;
    ;;iiii888888888888888888888800LLLL008888888800CCCCCC008888888800LLLL008888888888888888888888iiii;;
    ;;iiii888888888888888888880000000088888888GG;;;;;;;;;;GG888888880000000088888888888888888888iiii;;
    ;;iiii888888888888888888888800008888888888GGii;;;;;;iiGG888888888800008888888888888888888888iiii;;
    ;;ii;;0088888888888888888888888888888888888888GGCCGG8888888888888888888888888888888888888800;;ii;;
    ::ii;;LL8888888888888888888888888888888888888888888888888888888888888888888888888888888888LL;;ii::
    ,,;;;;;;LL888888888888888888888888888888888888888888888888888888888888888888888888888888LL;;;;;;,,
    ..;;;;;;;;11tttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttt11;;;;;;;;..
      ..;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;..
        ..::;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;::..
              ..............................................................................
                                    ..................................
"#;

const FETCH_MASCOT_SMALL: &str = r#"
       .;;;;;;;;;;;;;.
    .;1ttfffffffftt1;.
   ;1fLLCCCCCCCCLLf1;
  ;1C88@@@@@@@@@@88C1;
 ;1C88  >      <  88C1;
 ;1C88   |    |   88C1;
 ;1C88    \__/    88C1;
  ;1C888888888888C1;
   ;1fLLCCCCCCCCf1;
    .;1tttttttt1;.
       ';;;;;;;'
"#;

fn fetch(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "fetch — muestra información del sistema con la mascota de Shell Shock Tool\n\
             uso: fetch [--small|--full]\n\
             alias: neofetch, fastfetch\n",
        ));
    }

    let mut system = System::new_all();
    system.refresh_all();

    let user = env::var("USERNAME").unwrap_or_else(|_| "user".to_owned());
    let host = System::host_name().unwrap_or_else(|| "windows".to_owned());
    let os = System::long_os_version().unwrap_or_else(|| "Windows".to_owned());
    let kernel = System::kernel_version().unwrap_or_else(|| "desconocido".to_owned());
    let arch = env::consts::ARCH;
    let uptime = System::uptime();
    let days = uptime / 86_400;
    let hours = (uptime % 86_400) / 3_600;
    let minutes = (uptime % 3_600) / 60;
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{} cores", system.cpus().len()));
    let used = system.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let total = system.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let uptime_text = if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    };

    let small = args.iter().any(|arg| arg == "--small");
    let mascot = if small {
        FETCH_MASCOT_SMALL
    } else {
        FETCH_MASCOT_FULL
    };

    let mut out = String::new();
    out.push_str("\x1b[38;5;117m");
    out.push_str(mascot.trim_matches('\n'));
    out.push_str("\x1b[0m\n\n");

    let info = [
        format!("\x1b[1;38;5;203m{user}@{host}\x1b[0m"),
        "\x1b[38;5;244m────────────────────────────────────────\x1b[0m".to_owned(),
        format!("\x1b[1;38;5;222mOS:\x1b[0m {os}"),
        format!("\x1b[1;38;5;222mHost:\x1b[0m {host}"),
        format!("\x1b[1;38;5;222mKernel:\x1b[0m {kernel}"),
        format!("\x1b[1;38;5;222mUptime:\x1b[0m {uptime_text}"),
        "\x1b[1;38;5;222mShell:\x1b[0m Shell Shock Tool".to_owned(),
        "\x1b[1;38;5;222mBinary:\x1b[0m adm-tool".to_owned(),
        "\x1b[1;38;5;222mTerminal:\x1b[0m Shell Shock Native Terminal".to_owned(),
        format!("\x1b[1;38;5;222mCPU:\x1b[0m {cpu}"),
        format!("\x1b[1;38;5;222mMemory:\x1b[0m {used:.2} GiB / {total:.2} GiB"),
        format!("\x1b[1;38;5;222mArch:\x1b[0m {arch}"),
    ];

    for line in info {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str(
        "\x1b[48;5;203m  \x1b[48;5;117m  \x1b[48;5;222m  \
         \x1b[48;5;42m  \x1b[48;5;39m  \x1b[48;5;99m  \
         \x1b[48;5;250m  \x1b[48;5;255m  \x1b[0m\n",
    );

    Ok(CommandOutput::ok(out))
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
