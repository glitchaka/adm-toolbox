use super::{CommandOutput, net};

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    match args.first().map(String::as_str).unwrap_or("network") {
        "network" => net::diagnose_network(),
        "traffic" => net::run(&["traffic".to_owned()]),
        other => Ok(CommandOutput::error(
            format!("diag: tema no implementado: {other}"),
            2,
        )),
    }
}
