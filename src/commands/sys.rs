use sysinfo::{Disks, System};

use super::CommandOutput;

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("info");

    match sub {
        "info" => info(),
        "processes" => processes(),
        "disks" => disks(),
        "memory" => memory(),
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
    let mut out = String::from("MOUNT                 TOTAL GiB   FREE GiB   FS\n");

    for disk in disks.list() {
        out.push_str(&format!(
            "{:<20} {:>9.1} {:>10.1}   {}\n",
            disk.mount_point().display(),
            disk.total_space() as f64 / 1024.0 / 1024.0 / 1024.0,
            disk.available_space() as f64 / 1024.0 / 1024.0 / 1024.0,
            disk.file_system().to_string_lossy()
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn memory() -> anyhow::Result<CommandOutput> {
    let mut system = System::new_all();
    system.refresh_memory();

    Ok(CommandOutput::ok(format!(
        "total: {} MiB\nused: {} MiB\navailable: {} MiB\nswap_total: {} MiB\nswap_used: {} MiB\n",
        system.total_memory() / 1024 / 1024,
        system.used_memory() / 1024 / 1024,
        system.available_memory() / 1024 / 1024,
        system.total_swap() / 1024 / 1024,
        system.used_swap() / 1024 / 1024,
    )))
}
