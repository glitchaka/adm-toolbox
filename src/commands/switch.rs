use super::CommandOutput;

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    match args.first().map(String::as_str).unwrap_or("capabilities") {
        "capabilities" => Ok(CommandOutput::ok(
            "provider: none\nfdb: unavailable\nsnmp: not-configured\nlldp: not-configured\nport-locate: unavailable\n",
        )),
        "locate" => Ok(CommandOutput::error(
            "switch locate: configura un proveedor SNMP/API/SSH de solo lectura",
            2,
        )),
        other => Ok(CommandOutput::error(
            format!("switch: subcomando desconocido: {other}"),
            2,
        )),
    }
}
