use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;

use crate::core::{
    CommandOutput,
    ports::ProcessRunner,
};

pub struct NetworkDiagnosticsService {
    process: Arc<dyn ProcessRunner>,
}

impl NetworkDiagnosticsService {
    pub fn new(process: Arc<dyn ProcessRunner>) -> Self {
        Self { process }
    }

    pub fn diagnose(&self) -> Result<CommandOutput> {
        let mut out = String::new();
        out.push_str("== interfaces ==\n");
        out.push_str(&self.interfaces()?.stdout);
        out.push_str("\n== vecinos ==\n");
        out.push_str(&self.arp_table()?.stdout);
        out.push_str("\n== conexiones ==\n");
        out.push_str(&self.connections()?.stdout);
        Ok(CommandOutput::ok(out))
    }

    pub fn interfaces(&self) -> Result<CommandOutput> {
        self.run_command("ipconfig", &["/all"])
    }

    pub fn connections(&self) -> Result<CommandOutput> {
        self.run_command("netstat", &["-ano"])
    }

    pub fn routes(&self) -> Result<CommandOutput> {
        self.run_command("route", &["print"])
    }

    pub fn dns(&self, args: &[String]) -> Result<CommandOutput> {
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

    pub fn ping(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error("net ping: uso: net ping HOST", 2));
        };

        let count = args
            .windows(2)
            .find(|pair| pair[0] == "-c")
            .map(|pair| pair[1].as_str())
            .unwrap_or("4");

        self.run_command("ping", &["-n", count, target])
    }

    pub fn trace(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error("net trace: uso: net trace HOST", 2));
        };

        self.run_command("tracert", &["-d", target])
    }

    pub fn arp_map(&self) -> Result<HashMap<Ipv4Addr, String>> {
        let output = self.process.run("arp", &["-a"])?;
        let mut map = HashMap::new();

        for line in output.stdout.lines() {
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

    pub fn arp_table(&self) -> Result<CommandOutput> {
        let mut rows: Vec<_> = self.arp_map()?.into_iter().collect();
        rows.sort_by_key(|row| row.0);

        let mut out = String::from("IP               MAC\n");
        for (ip, mac) in rows {
            out.push_str(&format!("{:<16} {mac}\n", ip));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn ports(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(host) = args.first() else {
            return Ok(CommandOutput::error(
                "net ports: uso: net ports HOST 22,80,443",
                2,
            ));
        };

        let port_text = args.get(1).map(String::as_str).unwrap_or("22,80,443,445,3389");
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
                .and_then(|socket| TcpStream::connect_timeout(&socket, Duration::from_millis(500)).ok())
                .is_some();

            out.push_str(&format!(
                "{:<8} {}\n",
                port,
                if open { "open" } else { "closed/filtered" }
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn host_alive(&self, ip: Ipv4Addr, timeout: Duration) -> bool {
        for port in [445_u16, 3389, 80, 443, 135, 22] {
            if TcpStream::connect_timeout(&SocketAddr::new(IpAddr::V4(ip), port), timeout).is_ok() {
                return true;
            }
        }

        let timeout_ms = timeout.as_millis().to_string();
        let ip_text = ip.to_string();

        self.process
            .run("ping", &["-n", "1", "-w", &timeout_ms, &ip_text])
            .map(|output| output.status == 0)
            .unwrap_or(false)
    }

    pub fn default_ipv4_network(&self) -> Result<Ipv4Net> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect("8.8.8.8:80")?;

        let IpAddr::V4(ip) = socket.local_addr()?.ip() else {
            anyhow::bail!("no se pudo determinar una IPv4 local");
        };

        Ok(Ipv4Net::new(ip, 24)?)
    }

    fn run_command(&self, command: &str, args: &[&str]) -> Result<CommandOutput> {
        let output = self.process.run(command, args)?;

        Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            status: output.status,
        })
    }
}
