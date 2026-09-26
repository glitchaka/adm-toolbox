use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    io::{Write, stdout},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;
use serde::Serialize;
use sysinfo::System;

use super::{CommandOutput, device};

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("interfaces");

    match sub {
        "interfaces" => interfaces(),
        "connections" => connections(),
        "routes" => routes(),
        "dns" => dns(&args[1..]),
        "ping" => ping(&args[1..]),
        "trace" | "traceroute" => trace(&args[1..]),
        "scan" => scan(&args[1..]),
        "monitor" => monitor(&args[1..]),
        "neighbors" | "arp" => arp_table(),
        "ports" => ports(&args[1..]),
        "traffic" => traffic(&args[1..]),
        "usage" => usage(&args[1..]),
        "provider" => provider(&args[1..]),
        _ => Ok(CommandOutput::error(
            format!("net: subcomando desconocido: {sub}"),
            2,
        )),
    }
}

pub fn diagnose_network() -> anyhow::Result<CommandOutput> {
    let mut out = String::new();
    out.push_str("== interfaces ==\n");
    out.push_str(&interfaces()?.stdout);
    out.push_str("\n== vecinos ==\n");
    out.push_str(&arp_table()?.stdout);
    out.push_str("\n== conexiones ==\n");
    out.push_str(&connections()?.stdout);
    Ok(CommandOutput::ok(out))
}

fn interfaces() -> anyhow::Result<CommandOutput> {
    command_output("ipconfig", &["/all"])
}

fn connections() -> anyhow::Result<CommandOutput> {
    command_output("netstat", &["-ano"])
}

fn routes() -> anyhow::Result<CommandOutput> {
    command_output("route", &["print"])
}

fn dns(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(target) = args.first() else {
        return Ok(CommandOutput::error("net dns: uso: net dns HOST|IP", 2));
    };

    if let Ok(ip) = target.parse::<IpAddr>() {
        let name = lookup_addr(&ip).unwrap_or_else(|_| "-".to_owned());
        return Ok(CommandOutput::ok(format!(
            "address: {ip}\nname: {name}\n"
        )));
    }

    let mut addresses: Vec<IpAddr> = (target.as_str(), 0)
        .to_socket_addrs()?
        .map(|socket| socket.ip())
        .collect();

    addresses.sort();
    addresses.dedup();

    if addresses.is_empty() {
        return Ok(CommandOutput::error(
            format!("net dns: no se pudo resolver {target}"),
            1,
        ));
    }

    let mut out = format!("name: {target}\n");
    for address in addresses {
        out.push_str(&format!("address: {address}\n"));
    }

    Ok(CommandOutput::ok(out))
}

fn ping(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(target) = args.first() else {
        return Ok(CommandOutput::error("net ping: uso: net ping HOST", 2));
    };

    let count = args
        .windows(2)
        .find(|pair| pair[0] == "-c")
        .map(|pair| pair[1].as_str())
        .unwrap_or("4");

    command_output("ping", &["-n", count, target])
}

fn trace(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(target) = args.first() else {
        return Ok(CommandOutput::error("net trace: uso: net trace HOST", 2));
    };

    command_output("tracert", &["-d", target])
}

#[derive(Debug, Clone, Serialize)]
struct ScanRow {
    ip: Ipv4Addr,
    mac: String,
    hostname: String,
    latency_ms: u128,
    known: bool,
    inventory_name: Option<String>,
}

fn scan(args: &[String]) -> anyhow::Result<CommandOutput> {
    let network = if let Some(value) = args.iter().find(|arg| !arg.starts_with('-')) {
        value.parse::<Ipv4Net>()?
    } else {
        default_ipv4_network()?
    };

    if network.prefix_len() < 20 {
        return Ok(CommandOutput::error(
            "net scan: la primera versión limita el escaneo a /20 o redes más pequeñas",
            2,
        ));
    }

    let only_unknown = args.iter().any(|arg| arg == "--unknown");
    let only_known = args.iter().any(|arg| arg == "--authorized" || arg == "--known");
    let json = args.iter().any(|arg| arg == "--json");
    let csv = args.iter().any(|arg| arg == "--csv");

    let mut rows = scan_rows(network)?;

    if only_unknown {
        rows.retain(|row| !row.known);
    } else if only_known {
        rows.retain(|row| row.known);
    }

    if json {
        return Ok(CommandOutput::ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&rows)?
        )));
    }

    if csv {
        let mut out = String::from("ip,mac,hostname,latency_ms,known,inventory_name\n");
        for row in rows {
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                row.ip,
                row.mac,
                csv_escape(&row.hostname),
                row.latency_ms,
                row.known,
                csv_escape(row.inventory_name.as_deref().unwrap_or(""))
            ));
        }
        return Ok(CommandOutput::ok(out));
    }

    let mut out = String::from(
        "IP               MAC                 HOSTNAME                         LATENCY  INVENTORY\n",
    );

    for row in rows {
        let inventory = row
            .inventory_name
            .as_deref()
            .unwrap_or(if row.known { "known" } else { "unknown" });

        out.push_str(&format!(
            "{:<16} {:<19} {:<32} {:>4} ms  {}\n",
            row.ip, row.mac, row.hostname, row.latency_ms, inventory
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn scan_rows(network: Ipv4Net) -> anyhow::Result<Vec<ScanRow>> {
    let timeout = Duration::from_millis(300);
    let hosts: Vec<Ipv4Addr> = network.hosts().collect();
    let results: Arc<Mutex<Vec<(Ipv4Addr, Option<String>, Duration)>>> =
        Arc::new(Mutex::new(Vec::new()));

    for chunk in hosts.chunks(48) {
        let mut workers = Vec::new();

        for ip in chunk {
            let ip = *ip;
            let results = Arc::clone(&results);

            workers.push(thread::spawn(move || {
                let start = Instant::now();
                if host_alive(ip, timeout) {
                    let hostname = lookup_addr(&IpAddr::V4(ip)).ok();
                    results
                        .lock()
                        .unwrap()
                        .push((ip, hostname, start.elapsed()));
                }
            }));
        }

        for worker in workers {
            let _ = worker.join();
        }
    }

    let arp = parse_arp_map().unwrap_or_default();
    let known_names = device::known_names().unwrap_or_default();
    let known_macs = device::known_macs().unwrap_or_default();
    let mut discovered = results.lock().unwrap().clone();
    discovered.sort_by_key(|row| row.0);

    Ok(discovered
        .into_iter()
        .map(|(ip, hostname, latency)| {
            let mac = arp
                .get(&ip)
                .cloned()
                .unwrap_or_else(|| "??:??:??:??:??:??".to_owned());

            let inventory_name = known_names.get(&mac).cloned();
            let known = known_macs.contains(&mac);

            ScanRow {
                ip,
                mac,
                hostname: hostname.unwrap_or_else(|| "-".to_owned()),
                latency_ms: latency.as_millis(),
                known,
                inventory_name,
            }
        })
        .collect())
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn host_alive(ip: Ipv4Addr, timeout: Duration) -> bool {
    for port in [445_u16, 3389, 80, 443, 135, 22] {
        if TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(ip), port), timeout).is_ok() {
            return true;
        }
    }

    Command::new("ping")
        .args([
            "-n",
            "1",
            "-w",
            &timeout.as_millis().to_string(),
            &ip.to_string(),
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn default_ipv4_network() -> anyhow::Result<Ipv4Net> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;

    let IpAddr::V4(ip) = socket.local_addr()?.ip() else {
        anyhow::bail!("no se pudo determinar una IPv4 local");
    };

    Ok(Ipv4Net::new(ip, 24)?)
}

fn parse_arp_map() -> anyhow::Result<HashMap<Ipv4Addr, String>> {
    let output = Command::new("arp").arg("-a").output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();

    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() >= 2 {
            if let Ok(ip) = columns[0].parse::<Ipv4Addr>() {
                let mac = columns[1].replace('-', ":").to_ascii_uppercase();
                if mac.matches(':').count() == 5 {
                    map.insert(ip, mac);
                }
            }
        }
    }

    Ok(map)
}

fn arp_table() -> anyhow::Result<CommandOutput> {
    let map = parse_arp_map()?;
    let mut rows: Vec<_> = map.into_iter().collect();
    rows.sort_by_key(|row| row.0);

    let mut out = String::from("IP               MAC\n");
    for (ip, mac) in rows {
        out.push_str(&format!("{:<16} {mac}\n", ip));
    }

    Ok(CommandOutput::ok(out))
}

fn ports(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(host) = args.first() else {
        return Ok(CommandOutput::error(
            "net ports: uso: net ports HOST 22,80,443",
            2,
        ));
    };

    let port_text = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("22,80,443,445,3389");

    let ports: Vec<u16> = port_text
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .collect();

    let mut out = String::from("PORT     STATE\n");

    for port in ports {
        let address = format!("{host}:{port}");
        let socket = address
            .to_socket_addrs()
            .ok()
            .and_then(|mut values| values.next());

        let open = socket
            .and_then(|socket| {
                TcpStream::connect_timeout(&socket, Duration::from_millis(500)).ok()
            })
            .is_some();

        out.push_str(&format!(
            "{:<8} {}\n",
            port,
            if open { "open" } else { "closed/filtered" }
        ));
    }

    Ok(CommandOutput::ok(out))
}

#[derive(Debug, Clone, Serialize)]
struct TrafficRow {
    pid: u32,
    process: String,
    path: String,
    cpu_percent: f32,
    memory_mib: f64,
    connections: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ConnectionRow {
    protocol: String,
    local: String,
    remote: String,
    state: String,
    pid: u32,
    process: String,
}

fn traffic(args: &[String]) -> anyhow::Result<CommandOutput> {
    if args.iter().any(|arg| arg == "--watch" || arg == "-w") {
        return traffic_watch(args);
    }

    traffic_output(args)
}

fn traffic_output(args: &[String]) -> anyhow::Result<CommandOutput> {
    let connections = collect_connections()?;
    let mut system = System::new_all();
    system.refresh_all();

    let pid_filter = option_value(args, "--pid").and_then(|value| value.parse::<u32>().ok());
    let process_filter = option_value(args, "--process").map(str::to_ascii_lowercase);
    let top = option_value(args, "--top")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50);
    let json = args.iter().any(|arg| arg == "--json");
    let csv = args.iter().any(|arg| arg == "--csv");
    let show_connections = args.iter().any(|arg| arg == "--connections");

    if args.iter().any(|arg| arg == "--background") {
        return Ok(CommandOutput::error(
            "net traffic --background: la detección fiable de foreground/background se añadirá con el proveedor Win32 de ventanas",
            2,
        ));
    }

    let process_name_by_pid: HashMap<u32, String> = system
        .processes()
        .iter()
        .map(|(pid, process)| {
            (
                pid.as_u32(),
                process.name().to_string_lossy().into_owned(),
            )
        })
        .collect();

    if show_connections {
        let mut rows: Vec<ConnectionRow> = connections
            .into_iter()
            .map(|mut row| {
                row.process = process_name_by_pid
                    .get(&row.pid)
                    .cloned()
                    .unwrap_or_else(|| "-".to_owned());
                row
            })
            .filter(|row| {
                pid_filter.is_none_or(|pid| row.pid == pid)
                    && process_filter.as_ref().is_none_or(|needle| {
                        row.process.to_ascii_lowercase().contains(needle)
                    })
            })
            .collect();

        rows.sort_by(|a, b| {
            a.process
                .to_ascii_lowercase()
                .cmp(&b.process.to_ascii_lowercase())
                .then(a.pid.cmp(&b.pid))
        });

        if json {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&rows)?
            )));
        }

        if csv {
            let mut out =
                String::from("protocol,local,remote,state,pid,process\n");
            for row in rows {
                out.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    csv_escape(&row.protocol),
                    csv_escape(&row.local),
                    csv_escape(&row.remote),
                    csv_escape(&row.state),
                    row.pid,
                    csv_escape(&row.process)
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        let mut out = String::from(
            "PROTO  LOCAL                         REMOTE                        STATE          PID      PROCESS\n",
        );

        for row in rows.into_iter().take(top) {
            out.push_str(&format!(
                "{:<6} {:<29} {:<29} {:<14} {:<8} {}\n",
                row.protocol, row.local, row.remote, row.state, row.pid, row.process
            ));
        }

        return Ok(CommandOutput::ok(out));
    }

    let mut connection_counts: HashMap<u32, usize> = HashMap::new();
    for row in &connections {
        *connection_counts.entry(row.pid).or_insert(0) += 1;
    }

    let mut rows = Vec::new();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        let count = connection_counts.get(&pid_u32).copied().unwrap_or(0);

        if count == 0 {
            continue;
        }

        if pid_filter.is_some_and(|wanted| wanted != pid_u32) {
            continue;
        }

        let name = process.name().to_string_lossy().into_owned();

        if process_filter
            .as_ref()
            .is_some_and(|needle| !name.to_ascii_lowercase().contains(needle))
        {
            continue;
        }

        rows.push(TrafficRow {
            pid: pid_u32,
            process: name,
            path: process
                .exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            cpu_percent: process.cpu_usage(),
            memory_mib: process.memory() as f64 / 1024.0 / 1024.0,
            connections: count,
        });
    }

    rows.sort_by(|a, b| {
        b.connections.cmp(&a.connections).then_with(|| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    rows.truncate(top);

    if json {
        return Ok(CommandOutput::ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&rows)?
        )));
    }

    if csv {
        let mut out =
            String::from("pid,process,cpu_percent,memory_mib,connections,path\n");
        for row in rows {
            out.push_str(&format!(
                "{},{},{:.2},{:.2},{},{}\n",
                row.pid,
                csv_escape(&row.process),
                row.cpu_percent,
                row.memory_mib,
                row.connections,
                csv_escape(&row.path)
            ));
        }
        return Ok(CommandOutput::ok(out));
    }

    let mut out =
        String::from("PID      PROCESS                          CPU%     RAM MiB   CONNECTIONS\n");

    for row in rows {
        out.push_str(&format!(
            "{:<8} {:<32} {:>6.1} {:>10.1} {:>12}\n",
            row.pid, row.process, row.cpu_percent, row.memory_mib, row.connections
        ));
    }

    out.push_str(
        "\nUsa --connections para ver destinos. Bytes/s por PID se añadirá con ETW.\n",
    );

    Ok(CommandOutput::ok(out))
}

fn collect_connections() -> anyhow::Result<Vec<ConnectionRow>> {
    let output = Command::new("netstat").arg("-ano").output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();

    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();

        if columns.first().is_some_and(|value| value.eq_ignore_ascii_case("TCP"))
            && columns.len() >= 5
        {
            if let Ok(pid) = columns[4].parse::<u32>() {
                rows.push(ConnectionRow {
                    protocol: "TCP".to_owned(),
                    local: columns[1].to_owned(),
                    remote: columns[2].to_owned(),
                    state: columns[3].to_owned(),
                    pid,
                    process: String::new(),
                });
            }
        } else if columns
            .first()
            .is_some_and(|value| value.eq_ignore_ascii_case("UDP"))
            && columns.len() >= 4
        {
            if let Ok(pid) = columns[3].parse::<u32>() {
                rows.push(ConnectionRow {
                    protocol: "UDP".to_owned(),
                    local: columns[1].to_owned(),
                    remote: columns[2].to_owned(),
                    state: "-".to_owned(),
                    pid,
                    process: String::new(),
                });
            }
        }
    }

    Ok(rows)
}

fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn traffic_watch(args: &[String]) -> anyhow::Result<CommandOutput> {
    let _guard = NetTerminalGuard::enter()?;
    let filtered_args: Vec<String> = args
        .iter()
        .filter(|arg| arg.as_str() != "--watch" && arg.as_str() != "-w")
        .cloned()
        .collect();

    loop {
        execute!(
            stdout(),
            cursor::MoveTo(0, 0),
            Clear(ClearType::All)
        )?;

        println!("ADM net traffic --watch   [q] salir");
        println!();

        match traffic_output(&filtered_args) {
            Ok(output) => print!("{}", output.stdout),
            Err(error) => println!("error: {error}"),
        }

        stdout().flush()?;

        if event::poll(Duration::from_millis(1000))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }

    Ok(CommandOutput::ok(""))
}

fn monitor(_args: &[String]) -> anyhow::Result<CommandOutput> {
    let _guard = NetTerminalGuard::enter()?;

    loop {
        execute!(
            stdout(),
            cursor::MoveTo(0, 0),
            Clear(ClearType::All)
        )?;

        println!("ADM net monitor   red local /24   [q] salir");
        println!();

        match scan(&[]) {
            Ok(output) => print!("{}", output.stdout),
            Err(error) => println!("error: {error}"),
        }

        stdout().flush()?;

        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            if event::poll(Duration::from_millis(150))? {
                if let Event::Key(key) = event::read()? {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                        return Ok(CommandOutput::ok(""));
                    }
                }
            }
        }
    }
}

struct NetTerminalGuard;

impl NetTerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for NetTerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn usage(_args: &[String]) -> anyhow::Result<CommandOutput> {
    Ok(CommandOutput::error(
        "net usage: el gateway debe exponer contadores por cliente. Consulta 'net provider capabilities'.",
        2,
    ))
}

fn provider(args: &[String]) -> anyhow::Result<CommandOutput> {
    match args.first().map(String::as_str) {
        Some("capabilities") => Ok(CommandOutput::ok(
            "provider: none\ndevice-discovery: local\nper-device-traffic: no\nconnection-table: local\n",
        )),
        _ => Ok(CommandOutput::error(
            "net provider: no hay proveedor de gateway configurado",
            2,
        )),
    }
}

fn command_output(command: &str, args: &[&str]) -> anyhow::Result<CommandOutput> {
    let output = Command::new(command).args(args).output()?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code().unwrap_or(1),
    })
}
