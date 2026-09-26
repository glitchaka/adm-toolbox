use super::{CommandOutput, domain, net, sys};

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    match args.first().map(String::as_str).unwrap_or("network") {
        "network" => net::diagnose_network(),
        "dns" => diagnose_dns(&args[1..]),
        "hardware" => diagnose_hardware(),
        "storage" => diagnose_storage(),
        "traffic" => net::run(&["traffic".to_owned()]),
        "domain" => domain::run(&["status".to_owned()]),
        other => Ok(CommandOutput::error(
            format!("diag: tema no implementado: {other}"),
            2,
        )),
    }
}

fn diagnose_dns(args: &[String]) -> anyhow::Result<CommandOutput> {
    let target = args.first().cloned().unwrap_or_else(|| "example.com".to_owned());
    let mut out = String::from("== dns ==\n");
    out.push_str(&net::run(&["dns".to_owned(), target])?.stdout);
    Ok(CommandOutput::ok(out))
}

fn diagnose_hardware() -> anyhow::Result<CommandOutput> {
    let mut out = String::from("== system ==\n");
    out.push_str(&sys::run(&["info".to_owned()])?.stdout);
    out.push_str("\n== memory ==\n");
    out.push_str(&sys::run(&["memory".to_owned()])?.stdout);
    out.push_str("\n== disks ==\n");
    out.push_str(&sys::run(&["disks".to_owned()])?.stdout);
    Ok(CommandOutput::ok(out))
}

fn diagnose_storage() -> anyhow::Result<CommandOutput> {
    let mut out = String::from("== storage ==\n");
    out.push_str(&sys::run(&["disks".to_owned()])?.stdout);
    Ok(CommandOutput::ok(out))
}
