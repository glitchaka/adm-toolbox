use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use sysinfo::System;

use crate::{
    core::{
        CommandOutput,
        models::network::{ByteCounters, ConnectionRow, TrafficRow},
        ports::{
            ForegroundProcessProvider, NetworkProbe, TerminalFactory, TerminalKey,
            TrafficMonitorFactory,
        },
    },
    support::{csv, options},
};

pub struct NetworkTrafficService {
    probe: Arc<dyn NetworkProbe>,
    foreground: Arc<dyn ForegroundProcessProvider>,
    monitor_factory: Arc<dyn TrafficMonitorFactory>,
    terminal: Arc<dyn TerminalFactory>,
}

impl NetworkTrafficService {
    pub fn new(
        probe: Arc<dyn NetworkProbe>,
        foreground: Arc<dyn ForegroundProcessProvider>,
        monitor_factory: Arc<dyn TrafficMonitorFactory>,
        terminal: Arc<dyn TerminalFactory>,
    ) -> Self {
        Self {
            probe,
            foreground,
            monitor_factory,
            terminal,
        }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        if options::has(args, "--watch") || options::has(args, "-w") {
            return self.watch(args);
        }

        if options::has(args, "--connections") {
            return self.output(args, None, None);
        }

        match self.monitor_factory.start() {
            Ok(monitor) => {
                let before = monitor.snapshot();
                let started = Instant::now();
                std::thread::sleep(Duration::from_millis(1200));
                let after = monitor.snapshot();
                let rates = self
                    .monitor_factory
                    .rates(&before, &after, started.elapsed());

                self.output(args, Some(&rates), Some("ETW"))
            }
            Err(error) => self.output(
                args,
                None,
                Some(&format!("ETW no disponible: {error}")),
            ),
        }
    }

    fn output(
        &self,
        args: &[String],
        rate_map: Option<&HashMap<u32, ByteCounters>>,
        telemetry_note: Option<&str>,
    ) -> Result<CommandOutput> {
        let connections = self.collect_connections()?;
        let mut system = System::new_all();
        system.refresh_all();

        let pid_filter = options::value(args, "--pid").and_then(|value| value.parse::<u32>().ok());
        let process_filter = options::value(args, "--process").map(str::to_ascii_lowercase);
        let top = options::value(args, "--top")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50);
        let json = options::has(args, "--json");
        let csv_output = options::has(args, "--csv");
        let show_connections = options::has(args, "--connections");
        let background_only = options::has(args, "--background");
        let high_usage_only = options::has(args, "--high-usage");
        let foreground_pid = self.foreground.foreground_pid();

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

            if csv_output {
                let mut out = String::from("protocol,local,remote,state,pid,process\n");
                for row in rows {
                    out.push_str(&format!(
                        "{},{},{},{},{},{}\n",
                        csv::escape(&row.protocol),
                        csv::escape(&row.local),
                        csv::escape(&row.remote),
                        csv::escape(&row.state),
                        row.pid,
                        csv::escape(&row.process)
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
            let connection_count = connection_counts.get(&pid_u32).copied().unwrap_or(0);
            let rate = rate_map
                .and_then(|rates| rates.get(&pid_u32))
                .copied()
                .unwrap_or_default();

            if connection_count == 0 && rate.sent == 0 && rate.received == 0 {
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

            let foreground = foreground_pid == Some(pid_u32);
            if background_only && foreground {
                continue;
            }

            if high_usage_only
                && rate.sent.saturating_add(rate.received) < 100 * 1024
                && process.cpu_usage() < 10.0
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
                connections: connection_count,
                upload_bps: rate.sent,
                download_bps: rate.received,
                ppid: process.parent().map(|pid| pid.as_u32()),
                foreground,
            });
        }

        rows.sort_by(|a, b| {
            let a_rate = a.upload_bps.saturating_add(a.download_bps);
            let b_rate = b.upload_bps.saturating_add(b.download_bps);

            b_rate
                .cmp(&a_rate)
                .then(b.connections.cmp(&a.connections))
                .then_with(|| {
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

        if csv_output {
            let mut out = String::from(
                "pid,ppid,process,foreground,upload_bps,download_bps,cpu_percent,memory_mib,connections,path\n",
            );

            for row in rows {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{:.2},{:.2},{},{}\n",
                    row.pid,
                    row.ppid.map(|pid| pid.to_string()).unwrap_or_default(),
                    csv::escape(&row.process),
                    row.foreground,
                    row.upload_bps,
                    row.download_bps,
                    row.cpu_percent,
                    row.memory_mib,
                    row.connections,
                    csv::escape(&row.path)
                ));
            }

            return Ok(CommandOutput::ok(out));
        }

        let mut out = String::from(
            "PID      PPID     FG  PROCESS                 UP/s         DOWN/s       CPU%    RAM MiB   CONN\n",
        );

        for row in rows {
            out.push_str(&format!(
                "{:<8} {:<8} {:<3} {:<23} {:>11} {:>12} {:>6.1} {:>10.1} {:>6}\n",
                row.pid,
                row.ppid.map(|pid| pid.to_string()).unwrap_or_else(|| "-".to_owned()),
                if row.foreground { "yes" } else { "no" },
                truncate_text(&row.process, 23),
                format_rate(row.upload_bps),
                format_rate(row.download_bps),
                row.cpu_percent,
                row.memory_mib,
                row.connections
            ));
        }

        if let Some(note) = telemetry_note {
            out.push_str(&format!("\ntelemetry: {note}\n"));
        }

        out.push_str("Usa --connections para ver destinos y puertos.\n");
        Ok(CommandOutput::ok(out))
    }

    fn collect_connections(&self) -> Result<Vec<ConnectionRow>> {
        self.probe.connections()
    }

    fn watch(&self, args: &[String]) -> Result<CommandOutput> {
        let mut terminal = self.terminal.alternate_screen()?;
        let filtered_args: Vec<String> = args
            .iter()
            .filter(|arg| arg.as_str() != "--watch" && arg.as_str() != "-w")
            .cloned()
            .collect();

        let monitor = self.monitor_factory.start();
        let mut previous = monitor
            .as_ref()
            .ok()
            .map(|monitor| monitor.snapshot())
            .unwrap_or_default();
        let telemetry_error = monitor.as_ref().err().map(ToString::to_string);

        loop {
            let started = Instant::now();

            if wait_for_refresh_or_quit(
                terminal.as_mut(),
                Duration::from_secs(1),
            )? {
                break;
            }

            let rates = if let Ok(monitor) = &monitor {
                let current = monitor.snapshot();
                let result = self
                    .monitor_factory
                    .rates(&previous, &current, started.elapsed());
                previous = current;
                Some(result)
            } else {
                None
            };

            let telemetry_note = telemetry_error
                .as_ref()
                .map(|error| format!("ETW no disponible: {error}"))
                .unwrap_or_else(|| "ETW".to_owned());

            let mut screen = String::from("SST net traffic --watch   [q] salir\n\n");

            match self.output(&filtered_args, rates.as_ref(), Some(&telemetry_note)) {
                Ok(output) => screen.push_str(&output.stdout),
                Err(error) => {
                    screen.push_str("error: ");
                    screen.push_str(&error.to_string());
                    screen.push('\n');
                }
            }

            terminal.clear()?;
            terminal.write(&screen)?;
            terminal.flush()?;
        }

        Ok(CommandOutput::ok(""))
    }
}

fn wait_for_refresh_or_quit(
    terminal: &mut dyn crate::core::ports::TerminalSession,
    duration: Duration,
) -> Result<bool> {
    let started = Instant::now();

    while started.elapsed() < duration {
        if matches!(
            terminal.poll_key(Duration::from_millis(100))?,
            Some(TerminalKey::Char('q') | TerminalKey::Escape)
        ) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn format_rate(bytes_per_second: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    let value = bytes_per_second as f64;

    if value >= GIB {
        format!("{:.1} GiB/s", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MiB/s", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB/s", value / KIB)
    } else {
        format!("{} B/s", bytes_per_second)
    }
}

fn truncate_text(value: &str, width: usize) -> String {
    let count = value.chars().count();

    if count <= width {
        return value.to_owned();
    }

    if width <= 3 {
        return value.chars().take(width).collect();
    }

    let mut text: String = value.chars().take(width - 3).collect();
    text.push_str("...");
    text
}
