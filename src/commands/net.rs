use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;
use sysinfo::System;

use super::CommandOutput;

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("interfaces");

    match sub {
        "interfaces" => interfaces(),
        "connections" => connections(),
        "scan" => scan(&args[1..]),
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
    let mut rows = results.lock().unwrap().clone();
    rows.sort_by_key(|row| row.0);

    let mut out = String::from(
        "IP               MAC                 HOSTNAME                         LATENCY\n",
    );

    for (ip, hostname, latency) in rows {
        let mac = arp
            .get(&ip)
            .cloned()
            .unwrap_or_else(|| "??:??:??:??:??:??".to_owned());

        out.push_str(&format!(
            "{:<16} {:<19} {:<32} {:>4} ms\n",
            ip,
            mac,
            hostname.unwrap_or_else(|| "-".to_owned()),
            latency.as_millis()
        ));
    }

    Ok(CommandOutput::ok(out))
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

fn traffic(_args: &[String]) -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_all();

    let netstat = Command::new("netstat").arg("-ano").output()?;
    let text = String::from_utf8_lossy(&netstat.stdout);
    let mut pids: HashMap<u32, usize> = HashMap::new();

    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if let Some(pid) = columns.last().and_then(|value| value.parse::<u32>().ok()) {
            *pids.entry(pid).or_insert(0) += 1;
        }
    }

    let mut rows = Vec::new();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        let connections = pids.get(&pid_u32).copied().unwrap_or(0);

        if connections == 0 {
            continue;
        }

        rows.push((
            pid_u32,
            process.name().to_string_lossy().into_owned(),
            process.cpu_usage(),
            process.memory() as f64 / 1024.0 / 1024.0,
            connections,
        ));
    }

    rows.sort_by(|a, b| b.4.cmp(&a.4));

    let mut out = String::from(
        "PID      PROCESS                          CPU%     RAM MiB   CONNECTIONS\n",
    );

    for (pid, name, cpu, memory, connections) in rows {
        out.push_str(&format!(
            "{:<8} {:<32} {:>6.1} {:>10.1} {:>12}\n",
            pid, name, cpu, memory, connections
        ));
    }

    out.push_str(
        "\nFase actual: correlación proceso/conexión/CPU/RAM.\n\
         Bytes/s por PID se añadirá con TrafficMonitorService basado en ETW.\n",
    );

    Ok(CommandOutput::ok(out))
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
