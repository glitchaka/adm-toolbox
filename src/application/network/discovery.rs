use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use dns_lookup::lookup_addr;
use ipnet::Ipv4Net;

use crate::{
    core::{
        CommandOutput,
        models::network::{PresenceRecord, ScanRow},
        ports::{DeviceRepository, PresenceRepository, TerminalFactory, TerminalKey},
    },
    support::csv,
};

use super::NetworkDiagnosticsService;

pub struct NetworkDiscoveryService {
    diagnostics: Arc<NetworkDiagnosticsService>,
    devices: Arc<dyn DeviceRepository>,
    presence: Arc<dyn PresenceRepository>,
    terminal: Arc<dyn TerminalFactory>,
}

impl NetworkDiscoveryService {
    pub fn new(
        diagnostics: Arc<NetworkDiagnosticsService>,
        devices: Arc<dyn DeviceRepository>,
        presence: Arc<dyn PresenceRepository>,
        terminal: Arc<dyn TerminalFactory>,
    ) -> Self {
        Self {
            diagnostics,
            devices,
            presence,
            terminal,
        }
    }

    pub fn scan(&self, args: &[String]) -> Result<CommandOutput> {
        let network = self.network_from_args(args, "net scan")?;
        let only_unknown = args.iter().any(|arg| arg == "--unknown");
        let only_known = args.iter().any(|arg| arg == "--authorized" || arg == "--known");
        let json = args.iter().any(|arg| arg == "--json");
        let csv_output = args.iter().any(|arg| arg == "--csv");

        let mut rows = self.scan_rows(network)?;

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

        if csv_output {
            let mut out = String::from("ip,mac,hostname,latency_ms,known,inventory_name\n");
            for row in rows {
                out.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    row.ip,
                    row.mac,
                    csv::escape(&row.hostname),
                    row.latency_ms,
                    row.known,
                    csv::escape(row.inventory_name.as_deref().unwrap_or(""))
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        Ok(CommandOutput::ok(render_scan_rows(&rows)))
    }

    pub fn monitor(&self, args: &[String]) -> Result<CommandOutput> {
        let network = self.network_from_args(args, "net monitor")?;
        let only_unknown = args.iter().any(|arg| arg == "--unknown");
        let mut terminal = self.terminal.alternate_screen()?;
        let mut previous_online: HashMap<String, String> = HashMap::new();
        let mut history: HashMap<String, PresenceRecord> = self
            .presence
            .all()?
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        let mut events = Vec::new();

        loop {
            let mut rows = self.scan_rows(network)?;
            if only_unknown {
                rows.retain(|row| !row.known);
            }

            let now = Utc::now().to_rfc3339();
            let mut current_online = HashMap::new();

            for row in &rows {
                let id = scan_identity(row);
                current_online.insert(id.clone(), row.ip.to_string());

                match previous_online.get(&id) {
                    None => push_event(
                        &mut events,
                        format!(
                            "{} + {} {} {}",
                            timestamp_short(),
                            row.ip,
                            row.mac,
                            row.hostname
                        ),
                    ),
                    Some(previous_ip) if previous_ip != &row.ip.to_string() => push_event(
                        &mut events,
                        format!(
                            "{} ~ {} cambió IP {} -> {}",
                            timestamp_short(),
                            row.mac,
                            previous_ip,
                            row.ip
                        ),
                    ),
                    _ => {}
                }

                history
                    .entry(id.clone())
                    .and_modify(|record| {
                        record.mac = row.mac.clone();
                        record.ip = row.ip.to_string();
                        record.hostname = row.hostname.clone();
                        record.last_seen = now.clone();
                    })
                    .or_insert_with(|| PresenceRecord {
                        id,
                        mac: row.mac.clone(),
                        ip: row.ip.to_string(),
                        hostname: row.hostname.clone(),
                        first_seen: now.clone(),
                        last_seen: now.clone(),
                    });
            }

            for (id, ip) in &previous_online {
                if !current_online.contains_key(id) {
                    push_event(
                        &mut events,
                        format!("{} - {} {}", timestamp_short(), ip, id),
                    );
                }
            }

            previous_online = current_online;
            let records: Vec<_> = history.values().cloned().collect();
            self.presence.replace_all(&records)?;

            let mut screen = format!(
                "SST net monitor {}{}   [q] salir\n\n",
                network,
                if only_unknown { " --unknown" } else { "" }
            );
            screen.push_str(&render_scan_rows(&rows));
            screen.push_str("\nEventos recientes:\n");

            for event_line in &events {
                screen.push_str(event_line);
                screen.push('\n');
            }

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;

            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(3) {
                if matches!(
                    terminal.poll_key(Duration::from_millis(150))?,
                    Some(TerminalKey::Char('q') | TerminalKey::Escape)
                ) {
                    return Ok(CommandOutput::ok(""));
                }
            }
        }
    }

    pub fn presence(&self, args: &[String]) -> Result<CommandOutput> {
        let mut records = self.presence.all()?;
        records.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        if args.iter().any(|arg| arg == "--json") {
            return Ok(CommandOutput::ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&records)?
            )));
        }

        if args.iter().any(|arg| arg == "--csv") {
            let mut out = String::from("mac,ip,hostname,first_seen,last_seen\n");
            for record in records {
                out.push_str(&format!(
                    "{},{},{},{},{}\n",
                    csv::escape(&record.mac),
                    csv::escape(&record.ip),
                    csv::escape(&record.hostname),
                    csv::escape(&record.first_seen),
                    csv::escape(&record.last_seen)
                ));
            }
            return Ok(CommandOutput::ok(out));
        }

        let mut out =
            String::from("MAC                 IP               HOSTNAME                         LAST SEEN\n");

        for record in records {
            out.push_str(&format!(
                "{:<19} {:<16} {:<32} {}\n",
                record.mac, record.ip, record.hostname, record.last_seen
            ));
        }

        Ok(CommandOutput::ok(out))
    }

    pub fn scan_rows(&self, network: Ipv4Net) -> Result<Vec<ScanRow>> {
        let timeout = Duration::from_millis(300);
        let hosts: Vec<Ipv4Addr> = network.hosts().collect();
        let results: Arc<Mutex<Vec<(Ipv4Addr, Option<String>, Duration)>>> =
            Arc::new(Mutex::new(Vec::new()));

        for chunk in hosts.chunks(48) {
            let mut workers = Vec::new();

            for ip in chunk {
                let ip = *ip;
                let results = Arc::clone(&results);
                let diagnostics = Arc::clone(&self.diagnostics);

                workers.push(thread::spawn(move || {
                    let start = Instant::now();
                    if diagnostics.host_alive(ip, timeout) {
                        let hostname = lookup_addr(&IpAddr::V4(ip)).ok();
                        if let Ok(mut results) = results.lock() {
                            results.push((ip, hostname, start.elapsed()));
                        }
                    }
                }));
            }

            for worker in workers {
                let _ = worker.join();
            }
        }

        let arp = self.diagnostics.arp_map().unwrap_or_default();
        let devices = self.devices.all().unwrap_or_default();
        let known_names: HashMap<String, String> = devices
            .iter()
            .map(|device| (device.mac.clone(), device.name.clone()))
            .collect();
        let known_macs: HashSet<String> = devices
            .into_iter()
            .map(|device| device.mac)
            .collect();

        let mut discovered = results
            .lock()
            .map(|rows| rows.clone())
            .unwrap_or_default();
        discovered.sort_by_key(|row| row.0);

        Ok(discovered
            .into_iter()
            .map(|(ip, hostname, latency)| {
                let mac = arp
                    .get(&ip)
                    .cloned()
                    .unwrap_or_else(|| "??:??:??:??:??:??".to_owned());

                ScanRow {
                    ip,
                    inventory_name: known_names.get(&mac).cloned(),
                    known: known_macs.contains(&mac),
                    mac,
                    hostname: hostname.unwrap_or_else(|| "-".to_owned()),
                    latency_ms: latency.as_millis(),
                }
            })
            .collect())
    }

    fn network_from_args(&self, args: &[String], command: &str) -> Result<Ipv4Net> {
        let network = if let Some(value) = args.iter().find(|arg| !arg.starts_with('-')) {
            value.parse::<Ipv4Net>()?
        } else {
            self.diagnostics.default_ipv4_network()?
        };

        if network.prefix_len() < 20 {
            anyhow::bail!("{command}: el escaneo está limitado a /20 o redes más pequeñas");
        }

        Ok(network)
    }
}

fn render_scan_rows(rows: &[ScanRow]) -> String {
    let mut out =
        String::from("IP               MAC                 HOSTNAME                         LATENCY  INVENTORY\n");

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

    out
}

fn scan_identity(row: &ScanRow) -> String {
    if row.mac.starts_with("??") {
        format!("ip:{}", row.ip)
    } else {
        row.mac.clone()
    }
}

fn push_event(events: &mut Vec<String>, event: String) {
    events.push(event);
    if events.len() > 12 {
        events.remove(0);
    }
}

fn timestamp_short() -> String {
    Utc::now().format("[%H:%M:%S]").to_string()
}
