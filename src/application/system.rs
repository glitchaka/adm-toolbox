use std::{
    env,
    fmt::Write as _,
    process::Command,
    sync::Arc,
    time::Duration,
};

use sysinfo::{Disks, Pid, System};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError},
    System::Threading::{
        GetExitCodeProcess, OpenProcess, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    },
};

use crate::{
    core::{
        CommandOutput,
        ports::{TerminalFactory, TerminalKey},
    },
    support::options,
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
            "uptime" => uptime(),
            "services" => services(&args[1..]),
            "users" => users(&args[1..]),
            "drivers" => drivers(&args[1..]),
            "events" => events(&args[1..]),
            "registry" | "reg" => registry(&args[1..]),
            "tasks" | "scheduled-tasks" => scheduled_tasks(&args[1..]),
            "printer" | "printers" => printers(&args[1..]),
            "help" | "-h" | "--help" => Ok(system_help()),
            _ => Ok(CommandOutput::error(
                format!("sys: subcomando desconocido: {sub}\nusa 'sys --help' para ver los subcomandos disponibles"),
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
            "uptime" => uptime(),
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
                "SST top  uptime {}s  CPU {} cores  Mem {} / {} MiB",
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


fn system_help() -> CommandOutput {
    CommandOutput::ok(
        "sys — información y administración local de Windows\n\n\
         uso: sys SUBCOMANDO [opciones]\n\n\
         Información:\n\
           sys info                     resumen del equipo\n\
           sys processes                procesos por CPU/RAM\n\
           sys top                      monitor TUI de procesos\n\
           sys disks                    discos y uso\n\
           sys memory                   memoria y swap\n\
           sys uptime                   tiempo desde el último arranque\n\
           sys hostname                 nombre del equipo\n\
           sys whoami                   usuario actual\n\
           sys uname [-a]               identificación del sistema\n\
           sys kill PID [--tree]         termina un proceso o su árbol\n\n\
         Administración / auditoría de solo lectura:\n\
           sys services [NOMBRE]        servicios de Windows\n\
           sys users [USUARIO]          cuentas locales (o --domain)\n\
           sys drivers                  controladores instalados\n\
           sys drivers --pnp            paquetes de controladores PnP\n\
           sys drivers --devices        dispositivos PnP conectados\n\
           sys events [LOG]             últimos eventos (System por defecto)\n\
           sys registry CLAVE           consulta del Registro de Windows\n\
           sys tasks [NOMBRE]           tareas programadas\n\
           sys printer [NOMBRE]         impresoras instaladas o detalle de una impresora\n\n\
         Estas vistas usan las utilidades nativas de Windows como backend y no modifican\n\
         servicios, cuentas, registro, drivers, eventos ni tareas programadas.\n",
    )
}

fn uptime() -> anyhow::Result<CommandOutput> {
    let seconds = System::uptime();
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    Ok(CommandOutput::ok(format!(
        "uptime: {days}d {hours}h {minutes}m {secs}s\nseconds: {seconds}\n"
    )))
}

fn services(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys services — consulta servicios de Windows\n\
             uso:\n\
               sys services              todos los servicios\n\
               sys services --running    solo servicios activos\n\
               sys services --stopped    servicios detenidos\n\
               sys services NOMBRE       detalle de un servicio\n",
        ));
    }

    let mut native = vec!["query".to_owned()];
    if let Some(name) = args.first().filter(|arg| !arg.starts_with('-')) {
        native.push(name.clone());
    } else if options::has(args, "--running") {
        native.push("state=".to_owned());
        native.push("active".to_owned());
    } else if options::has(args, "--stopped") {
        native.push("state=".to_owned());
        native.push("inactive".to_owned());
    } else {
        native.push("state=".to_owned());
        native.push("all".to_owned());
    }

    run_windows_tool("sc.exe", &native)
}

fn users(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys users — consulta cuentas de Windows\n\
             uso:\n\
               sys users                  cuentas locales\n\
               sys users USUARIO          detalle de una cuenta\n\
               sys users --domain         cuentas del dominio\n\
               sys users USUARIO --domain detalle de cuenta de dominio\n",
        ));
    }

    let mut native = vec!["user".to_owned()];
    if let Some(name) = args.first().filter(|arg| !arg.starts_with('-')) {
        native.push(name.clone());
    }
    if options::has(args, "--domain") {
        native.push("/domain".to_owned());
    }

    run_windows_tool("net.exe", &native)
}

fn drivers(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys drivers — consulta controladores y dispositivos PnP\n\
             uso:\n\
               sys drivers                listado de drivers\n\
               sys drivers --verbose      información detallada\n\
               sys drivers --signed       información de firma\n\
               sys drivers --csv          salida CSV\n\
               sys drivers --pnp          paquetes de drivers PnP\n\
               sys drivers --devices      dispositivos PnP conectados\n",
        ));
    }

    if options::has(args, "--pnp") {
        return run_windows_tool("pnputil.exe", &["/enum-drivers".to_owned()]);
    }

    if options::has(args, "--devices") {
        return run_windows_tool(
            "pnputil.exe",
            &["/enum-devices".to_owned(), "/connected".to_owned()],
        );
    }

    let mut native = Vec::new();
    if options::has(args, "--verbose") {
        native.push("/V".to_owned());
    }
    if options::has(args, "--signed") {
        native.push("/SI".to_owned());
    }
    native.push("/FO".to_owned());
    native.push(if options::has(args, "--csv") {
        "CSV".to_owned()
    } else {
        "TABLE".to_owned()
    });

    run_windows_tool("driverquery.exe", &native)
}

fn events(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys events — consulta Windows Event Log\n\
             uso:\n\
               sys events                       últimos 20 eventos de System\n\
               sys events Application           últimos eventos de Application\n\
               sys events --log Security        selecciona un log\n\
               sys events --count 50            cantidad (1..500)\n\
               sys events --query XPATH         filtro XPath de wevtutil\n\
               sys events --format text|xml     formato de salida\n\
               sys events --logs                lista logs disponibles\n\
               sys events --publishers          lista publishers\n",
        ));
    }

    if options::has(args, "--logs") {
        return run_windows_tool("wevtutil.exe", &["el".to_owned()]);
    }
    if options::has(args, "--publishers") {
        return run_windows_tool("wevtutil.exe", &["ep".to_owned()]);
    }

    let positional_log = args.first().filter(|arg| !arg.starts_with('-')).map(String::as_str);
    let log = options::value(args, "--log")
        .or(positional_log)
        .unwrap_or("System");

    let count = options::value(args, "--count")
        .unwrap_or("20")
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("sys events: --count debe ser un número"))?;

    if !(1..=500).contains(&count) {
        return Ok(CommandOutput::error(
            "sys events: --count debe estar entre 1 y 500",
            2,
        ));
    }

    let format = options::value(args, "--format").unwrap_or("text");
    if !matches!(format, "text" | "xml") {
        return Ok(CommandOutput::error(
            "sys events: --format debe ser text o xml",
            2,
        ));
    }

    let mut native = vec![
        "qe".to_owned(),
        log.to_owned(),
        format!("/c:{count}"),
        "/rd:true".to_owned(),
        format!("/f:{format}"),
    ];

    if let Some(query) = options::value(args, "--query") {
        native.push(format!("/q:{query}"));
    }

    run_windows_tool("wevtutil.exe", &native)
}

fn registry(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.is_empty() || args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys registry — consulta de solo lectura del Registro de Windows\n\
             uso:\n\
               sys registry CLAVE\n\
               sys registry query CLAVE\n\
               sys registry CLAVE --value NOMBRE\n\
               sys registry CLAVE --default\n\
               sys registry CLAVE --recursive\n\
               sys registry CLAVE --find TEXTO [--keys|--data]\n\
             ejemplo:\n\
               sys registry \"HKLM\\\\SOFTWARE\\\\Microsoft\\\\Windows NT\\\\CurrentVersion\"\n",
        ));
    }

    let offset = if args.first().is_some_and(|arg| arg == "query") { 1 } else { 0 };
    let Some(key) = args.get(offset) else {
        return Ok(CommandOutput::error("sys registry: falta CLAVE", 2));
    };
    if key.starts_with('-') {
        return Ok(CommandOutput::error(
            "sys registry: la CLAVE debe ir antes de las opciones",
            2,
        ));
    }

    let mut native = vec!["query".to_owned(), key.clone()];

    if let Some(value) = options::value(args, "--value") {
        native.push("/v".to_owned());
        native.push(value.to_owned());
    } else if options::has(args, "--default") {
        native.push("/ve".to_owned());
    }

    if options::has(args, "--recursive") {
        native.push("/s".to_owned());
    }

    if let Some(find) = options::value(args, "--find") {
        native.push("/f".to_owned());
        native.push(find.to_owned());
        if options::has(args, "--keys") {
            native.push("/k".to_owned());
        }
        if options::has(args, "--data") {
            native.push("/d".to_owned());
        }
    }

    run_windows_tool("reg.exe", &native)
}

fn scheduled_tasks(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys tasks — consulta tareas programadas de Windows\n\
             uso:\n\
               sys tasks                  listado en tabla\n\
               sys tasks --verbose        listado detallado\n\
               sys tasks --csv            salida CSV\n\
               sys tasks NOMBRE           detalle de una tarea\n\
               sys tasks NOMBRE --xml     definición XML de una tarea\n",
        ));
    }

    let name = args.first().filter(|arg| !arg.starts_with('-'));
    let mut native = vec!["/Query".to_owned()];

    if let Some(name) = name {
        native.push("/TN".to_owned());
        native.push(name.clone());

        if options::has(args, "--xml") {
            native.push("/XML".to_owned());
            return run_windows_tool("schtasks.exe", &native);
        }

        native.push("/FO".to_owned());
        native.push(if options::has(args, "--csv") {
            "CSV".to_owned()
        } else {
            "LIST".to_owned()
        });
        native.push("/V".to_owned());
    } else {
        native.push("/FO".to_owned());
        native.push(if options::has(args, "--csv") {
            "CSV".to_owned()
        } else {
            "TABLE".to_owned()
        });
        if options::has(args, "--verbose") {
            native.push("/V".to_owned());
        }
    }

    run_windows_tool("schtasks.exe", &native)
}


fn printers(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys printer — impresoras instaladas en el equipo\n\
             uso:\n\
               sys printer                 muestra todas las impresoras\n\
               sys printer NOMBRE          muestra solo la impresora indicada\n\
             NOMBRE puede ser el nombre completo UNC o el nombre compartido.\n",
        ));
    }

    let requested = args
        .first()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str);

    let requested_ps = requested
        .map(|value| value.replace('\'', "''"))
        .unwrap_or_default();

    let filter = if requested.is_some() {
        format!(
            r#"
$needle = '{requested_ps}'
$printers = $printers | Where-Object {{
    $_.Name -ieq $needle -or
    $_.ShareName -ieq $needle -or
    (($_.Name -split '\\\\')[-1] -ieq $needle)
}}
if (-not $printers) {{
    [Console]::Error.WriteLine("sys printer: no se encontró la impresora: $needle")
    exit 2
}}
"#
        )
    } else {
        String::new()
    };

    let script = format!(
        r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$printers = @(Get-CimInstance Win32_Printer -ErrorAction Stop)
{filter}

function Get-PrinterIp([object]$printer) {{
    $portName = [string]$printer.PortName

    try {{
        $port = Get-PrinterPort -Name $portName -ErrorAction Stop
        if ($port.PrinterHostAddress) {{
            return [string]$port.PrinterHostAddress
        }}
    }} catch {{}}

    if ($portName -match '^(?:\d{{1,3}}\.){{3}}\d{{1,3}}
    let output = Command::new(program).args(args).output().map_err(|error| {
        anyhow::anyhow!("{program}: no se pudo ejecutar la utilidad nativa de Windows: {error}")
    })?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
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
        "\x1b[1;38;5;222mBinary:\x1b[0m sst".to_owned(),
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
            "SST-Windows {host} {kernel} {arch} {os}\n"
        )))
    } else {
        Ok(CommandOutput::ok("SST-Windows\n"))
    }
}

fn kill_process(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys kill — termina un proceso mediante la API Win32\n\
             uso:\n\
               sys kill PID          termina exactamente ese PID\n\
               sys kill PID --tree   termina el PID y sus descendientes\n"
        ));
    }

    let Some(pid_text) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return Ok(CommandOutput::error(
            "kill: uso: sys kill PID [--tree]",
            2,
        ));
    };

    let pid_value = pid_text
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

    let process_name = process.name().to_string_lossy().into_owned();
    let tree = args.iter().any(|arg| matches!(arg.as_str(), "--tree" | "-t"));

    if tree {
        let mut descendants = process_descendants(&system, pid);
        descendants.reverse();

        for child in descendants {
            if let Err(error) = terminate_pid_native(child.as_u32()) {
                return Ok(CommandOutput::error(
                    format!(
                        "kill: no se pudo terminar el proceso hijo {} de {}: {error}",
                        child.as_u32(),
                        pid_value
                    ),
                    1,
                ));
            }
        }
    }

    if let Err(error) = terminate_pid_native(pid_value) {
        return Ok(CommandOutput::error(
            format!("kill: no se pudo terminar {pid_value} ({process_name}): {error}"),
            1,
        ));
    }

    Ok(CommandOutput::ok(format!(
        "terminated: {} {}{}\n",
        pid_value,
        process_name,
        if tree { " (process tree)" } else { "" }
    )))
}

fn process_descendants(system: &System, root: Pid) -> Vec<Pid> {
    let mut result = Vec::new();
    let mut pending = vec![root];

    while let Some(parent) = pending.pop() {
        let children: Vec<Pid> = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| (process.parent() == Some(parent)).then_some(*pid))
            .collect();

        for child in children {
            result.push(child);
            pending.push(child);
        }
    }

    result
}

#[cfg(windows)]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        let handle = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );

        if handle.is_null() {
            anyhow::bail!("OpenProcess falló con error Win32 {}", GetLastError());
        }

        if TerminateProcess(handle, 1) == 0 {
            let error = GetLastError();
            CloseHandle(handle);
            anyhow::bail!("TerminateProcess falló con error Win32 {error}");
        }

        let mut terminated = false;
        for _ in 0..20 {
            let mut exit_code = STILL_ACTIVE_CODE;
            if GetExitCodeProcess(handle, &mut exit_code) == 0 {
                let error = GetLastError();
                CloseHandle(handle);
                anyhow::bail!("GetExitCodeProcess falló con error Win32 {error}");
            }

            if exit_code != STILL_ACTIVE_CODE {
                terminated = true;
                break;
            }

            std::thread::sleep(Duration::from_millis(25));
        }

        CloseHandle(handle);

        if !terminated {
            anyhow::bail!("el proceso sigue activo después de TerminateProcess");
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    anyhow::bail!("la terminación nativa por PID solo está disponible en Windows: {pid}")
}
) {{
        return $portName
    }}

    if ($portName -match 'IP_((?:\d{{1,3}}\.){{3}}\d{{1,3}})') {{
        return $Matches[1]
    }}

    return '-'
}}

foreach ($printer in $printers) {{
    $server = if ($printer.ServerName) {{ [string]$printer.ServerName }} else {{ '-' }}
    $share = if ($printer.ShareName) {{ [string]$printer.ShareName }} else {{ '-' }}
    $port = if ($printer.PortName) {{ [string]$printer.PortName }} else {{ '-' }}
    $driver = if ($printer.DriverName) {{ [string]$printer.DriverName }} else {{ '-' }}
    $location = if ($printer.Location) {{ [string]$printer.Location }} else {{ '-' }}
    $default = if ($printer.Default) {{ 'sí' }} else {{ 'no' }}
    $ip = Get-PrinterIp $printer

    Write-Output 'Impresora'
    Write-Output '------------------------------------------------------------------------'
    Write-Output ("Nombre:          " + [string]$printer.Name)
    Write-Output ("Predeterminada:  " + $default)
    Write-Output ("Servidor:        " + $server)
    Write-Output ("Compartida:      " + $share)
    Write-Output ("Puerto:          " + $port)
    Write-Output ("IP:              " + $ip)
    Write-Output ("Driver:          " + $driver)
    Write-Output ("Ubicación:       " + $location)
    Write-Output ''
}}
"#
    );

    run_windows_tool(
        "powershell.exe",
        &[
            "-NoProfile".to_owned(),
            "-NonInteractive".to_owned(),
            "-Command".to_owned(),
            script,
        ],
    )
}

fn run_windows_tool(program: &str, args: &[String]) -> anyhow::Result<CommandOutput> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        anyhow::anyhow!("{program}: no se pudo ejecutar la utilidad nativa de Windows: {error}")
    })?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
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
        "\x1b[1;38;5;222mBinary:\x1b[0m sst".to_owned(),
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
            "SST-Windows {host} {kernel} {arch} {os}\n"
        )))
    } else {
        Ok(CommandOutput::ok("SST-Windows\n"))
    }
}

fn kill_process(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help")) {
        return Ok(CommandOutput::ok(
            "sys kill — termina un proceso mediante la API Win32\n\
             uso:\n\
               sys kill PID          termina exactamente ese PID\n\
               sys kill PID --tree   termina el PID y sus descendientes\n"
        ));
    }

    let Some(pid_text) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return Ok(CommandOutput::error(
            "kill: uso: sys kill PID [--tree]",
            2,
        ));
    };

    let pid_value = pid_text
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

    let process_name = process.name().to_string_lossy().into_owned();
    let tree = args.iter().any(|arg| matches!(arg.as_str(), "--tree" | "-t"));

    if tree {
        let mut descendants = process_descendants(&system, pid);
        descendants.reverse();

        for child in descendants {
            if let Err(error) = terminate_pid_native(child.as_u32()) {
                return Ok(CommandOutput::error(
                    format!(
                        "kill: no se pudo terminar el proceso hijo {} de {}: {error}",
                        child.as_u32(),
                        pid_value
                    ),
                    1,
                ));
            }
        }
    }

    if let Err(error) = terminate_pid_native(pid_value) {
        return Ok(CommandOutput::error(
            format!("kill: no se pudo terminar {pid_value} ({process_name}): {error}"),
            1,
        ));
    }

    Ok(CommandOutput::ok(format!(
        "terminated: {} {}{}\n",
        pid_value,
        process_name,
        if tree { " (process tree)" } else { "" }
    )))
}

fn process_descendants(system: &System, root: Pid) -> Vec<Pid> {
    let mut result = Vec::new();
    let mut pending = vec![root];

    while let Some(parent) = pending.pop() {
        let children: Vec<Pid> = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| (process.parent() == Some(parent)).then_some(*pid))
            .collect();

        for child in children {
            result.push(child);
            pending.push(child);
        }
    }

    result
}

#[cfg(windows)]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        let handle = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );

        if handle.is_null() {
            anyhow::bail!("OpenProcess falló con error Win32 {}", GetLastError());
        }

        if TerminateProcess(handle, 1) == 0 {
            let error = GetLastError();
            CloseHandle(handle);
            anyhow::bail!("TerminateProcess falló con error Win32 {error}");
        }

        let mut terminated = false;
        for _ in 0..20 {
            let mut exit_code = STILL_ACTIVE_CODE;
            if GetExitCodeProcess(handle, &mut exit_code) == 0 {
                let error = GetLastError();
                CloseHandle(handle);
                anyhow::bail!("GetExitCodeProcess falló con error Win32 {error}");
            }

            if exit_code != STILL_ACTIVE_CODE {
                terminated = true;
                break;
            }

            std::thread::sleep(Duration::from_millis(25));
        }

        CloseHandle(handle);

        if !terminated {
            anyhow::bail!("el proceso sigue activo después de TerminateProcess");
        }
    }

    Ok(())
}

#[cfg(not(windows))]
fn terminate_pid_native(pid: u32) -> anyhow::Result<()> {
    anyhow::bail!("la terminación nativa por PID solo está disponible en Windows: {pid}")
}
