use std::{
    env, fs,
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use snmp2::{Oid, SyncSession, Value};

use super::{CommandOutput, device};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SwitchProfile {
    name: String,
    host: String,
    community_env: String,
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct LocatedPort {
    switch: String,
    host: String,
    mac: String,
    vlan: Option<u32>,
    bridge_port: i64,
    if_index: i64,
    interface: String,
    alias: String,
    pvid: Option<i64>,
    speed_mbps: Option<i64>,
    oper_status: String,
}

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    match args.first().map(String::as_str).unwrap_or("list") {
        "list" => list(&args[1..]),
        "add" => add(&args[1..]),
        "remove" => remove(&args[1..]),
        "show" => show(&args[1..]),
        "path" => Ok(CommandOutput::ok(format!(
            "{}\n",
            database_path().display()
        ))),
        "capabilities" => capabilities(),
        "locate" => locate(&args[1..]),
        other => Ok(CommandOutput::error(
            format!("switch: subcomando desconocido: {other}"),
            2,
        )),
    }
}

fn list(args: &[String]) -> anyhow::Result<CommandOutput> {
    let switches = load()?;

    if args.iter().any(|arg| arg == "--json") {
        return Ok(CommandOutput::ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&switches)?
        )));
    }

    let mut out = String::from("NAME                 HOST                    COMMUNITY ENV\n");
    for switch in switches {
        out.push_str(&format!(
            "{:<20} {:<23} {}\n",
            switch.name, switch.host, switch.community_env
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn add(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(name) = args.first() else {
        return Ok(CommandOutput::error(
            "switch add: uso: switch add NOMBRE --host HOST --community-env VARIABLE",
            2,
        ));
    };

    let Some(host) = option_value(args, "--host") else {
        return Ok(CommandOutput::error("switch add: falta --host", 2));
    };

    let Some(community_env) = option_value(args, "--community-env") else {
        return Ok(CommandOutput::error(
            "switch add: falta --community-env; la comunidad SNMP no se guarda en texto plano",
            2,
        ));
    };

    let description = option_value(args, "--description").map(str::to_owned);
    let mut switches = load()?;

    if let Some(existing) = switches.iter_mut().find(|switch| switch.name == *name) {
        existing.host = host.to_owned();
        existing.community_env = community_env.to_owned();
        existing.description = description;
    } else {
        switches.push(SwitchProfile {
            name: name.clone(),
            host: host.to_owned(),
            community_env: community_env.to_owned(),
            description,
        });
    }

    switches.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    save(&switches)?;

    Ok(CommandOutput::ok(format!(
        "switch saved: {} {}\n",
        name, host
    )))
}

fn remove(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(name) = args.first() else {
        return Ok(CommandOutput::error("switch remove: falta nombre", 2));
    };

    let mut switches = load()?;
    let before = switches.len();
    switches.retain(|switch| switch.name != *name);

    if switches.len() == before {
        return Ok(CommandOutput::error(
            format!("switch: no existe {name}"),
            1,
        ));
    }

    save(&switches)?;
    Ok(CommandOutput::ok(format!("switch removed: {name}\n")))
}

fn show(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(name) = args.first() else {
        return Ok(CommandOutput::error("switch show: falta nombre", 2));
    };

    let switches = load()?;
    let Some(switch) = switches.iter().find(|switch| switch.name == *name) else {
        return Ok(CommandOutput::error(
            format!("switch: no existe {name}"),
            1,
        ));
    };

    Ok(CommandOutput::ok(format!(
        "name: {}\nhost: {}\ncommunity_env: {}\ndescription: {}\n",
        switch.name,
        switch.host,
        switch.community_env,
        switch.description.as_deref().unwrap_or("-")
    )))
}

fn capabilities() -> anyhow::Result<CommandOutput> {
    Ok(CommandOutput::ok(
        "provider: snmp2-rust\nprotocol: SNMPv2c\nmode: read-only\nbridge-mib: yes\nq-bridge-mib: VLAN lookup when --vlan is supplied\nif-mib: interface name, alias, speed, status\ncredentials: environment variable reference\n",
    ))
}

fn locate(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(target) = args.first() else {
        return Ok(CommandOutput::error(
            "switch locate: uso: switch locate MAC|NOMBRE [--switch NOMBRE] [--vlan N]",
            2,
        ));
    };

    let mac_text = resolve_mac(target)?;
    let mac = parse_mac(&mac_text)?;
    let selected_switch = option_value(args, "--switch");
    let vlan = option_value(args, "--vlan").and_then(|value| value.parse::<u32>().ok());
    let json = args.iter().any(|arg| arg == "--json");

    let switches = load()?;
    if switches.is_empty() {
        return Ok(CommandOutput::error(
            "switch locate: no hay switches configurados; usa 'switch add'",
            2,
        ));
    }

    let candidates: Vec<&SwitchProfile> = switches
        .iter()
        .filter(|switch| selected_switch.is_none_or(|name| switch.name == name))
        .collect();

    if candidates.is_empty() {
        return Ok(CommandOutput::error(
            "switch locate: el switch solicitado no está configurado",
            1,
        ));
    }

    let mut errors = Vec::new();

    for switch in candidates {
        match locate_on_switch(switch, mac, &mac_text, vlan) {
            Ok(Some(port)) => {
                if json {
                    return Ok(CommandOutput::ok(format!(
                        "{}\n",
                        serde_json::to_string_pretty(&port)?
                    )));
                }

                return Ok(CommandOutput::ok(format!(
                    "switch: {}\nhost: {}\nmac: {}\nvlan: {}\nbridge_port: {}\nif_index: {}\ninterface: {}\nalias: {}\npvid: {}\nspeed_mbps: {}\nstatus: {}\n",
                    port.switch,
                    port.host,
                    port.mac,
                    port.vlan.map(|value| value.to_string()).unwrap_or_else(|| "-".to_owned()),
                    port.bridge_port,
                    port.if_index,
                    port.interface,
                    if port.alias.is_empty() { "-" } else { &port.alias },
                    port.pvid.map(|value| value.to_string()).unwrap_or_else(|| "-".to_owned()),
                    port.speed_mbps.map(|value| value.to_string()).unwrap_or_else(|| "-".to_owned()),
                    port.oper_status
                )));
            }
            Ok(None) => {}
            Err(error) => errors.push(format!("{}: {error}", switch.name)),
        }
    }

    if !errors.is_empty() {
        return Ok(CommandOutput::error(
            format!(
                "switch locate: MAC no encontrada; consultas con error:\n{}",
                errors.join("\n")
            ),
            1,
        ));
    }

    Ok(CommandOutput::error(
        format!("switch locate: {} no aparece en los switches consultados", mac_text),
        1,
    ))
}

fn locate_on_switch(
    profile: &SwitchProfile,
    mac: [u8; 6],
    mac_text: &str,
    vlan: Option<u32>,
) -> anyhow::Result<Option<LocatedPort>> {
    let community = env::var(&profile.community_env).map_err(|_| {
        anyhow::anyhow!(
            "falta la variable de entorno {} con la comunidad SNMP",
            profile.community_env
        )
    })?;

    let address = snmp_address(&profile.host);
    let mut session = SyncSession::new_v2c(
        address.as_str(),
        community.as_bytes(),
        Some(Duration::from_secs(2)),
        0,
    )
    .map_err(|error| anyhow::anyhow!("SNMP: {error}"))?;

    let bridge_port = if let Some(vlan) = vlan {
        let mut oid = vec![1, 3, 6, 1, 2, 1, 17, 7, 1, 2, 2, 1, 2, vlan];
        oid.extend(mac.iter().map(|byte| *byte as u32));
        snmp_get_integer(&mut session, &oid)?
    } else {
        let mut oid = vec![1, 3, 6, 1, 2, 1, 17, 4, 3, 1, 2];
        oid.extend(mac.iter().map(|byte| *byte as u32));
        snmp_get_integer(&mut session, &oid)?
    };

    let Some(bridge_port) = bridge_port else {
        return Ok(None);
    };

    let if_index = snmp_get_integer(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 17, 1, 4, 1, 2, bridge_port as u32],
    )?
    .unwrap_or(bridge_port);

    let interface = snmp_get_string(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 1, if_index as u32],
    )?
    .or_else(|| {
        snmp_get_string(
            &mut session,
            &[1, 3, 6, 1, 2, 1, 2, 2, 1, 2, if_index as u32],
        )
        .ok()
        .flatten()
    })
    .unwrap_or_else(|| format!("ifIndex {if_index}"));

    let alias = snmp_get_string(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 18, if_index as u32],
    )?
    .unwrap_or_default();

    let pvid = snmp_get_integer(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 17, 7, 1, 4, 5, 1, 1, bridge_port as u32],
    )
    .ok()
    .flatten();

    let speed_mbps = snmp_get_integer(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 31, 1, 1, 1, 15, if_index as u32],
    )
    .ok()
    .flatten();

    let oper_status = match snmp_get_integer(
        &mut session,
        &[1, 3, 6, 1, 2, 1, 2, 2, 1, 8, if_index as u32],
    )
    .ok()
    .flatten()
    {
        Some(1) => "up".to_owned(),
        Some(2) => "down".to_owned(),
        Some(3) => "testing".to_owned(),
        Some(4) => "unknown".to_owned(),
        Some(5) => "dormant".to_owned(),
        Some(6) => "notPresent".to_owned(),
        Some(7) => "lowerLayerDown".to_owned(),
        Some(value) => value.to_string(),
        None => "-".to_owned(),
    };

    Ok(Some(LocatedPort {
        switch: profile.name.clone(),
        host: profile.host.clone(),
        mac: mac_text.to_owned(),
        vlan,
        bridge_port,
        if_index,
        interface,
        alias,
        pvid,
        speed_mbps,
        oper_status,
    }))
}

fn snmp_get_integer(
    session: &mut SyncSession,
    components: &[u32],
) -> anyhow::Result<Option<i64>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;

    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };

    match value {
        Value::Integer(value) => Ok(Some(value)),
        Value::Unsigned32(value) => Ok(Some(value as i64)),
        Value::Counter32(value) => Ok(Some(value as i64)),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        other => Err(anyhow::anyhow!(
            "SNMP devolvió un tipo inesperado: {other:?}"
        )),
    }
}

fn snmp_get_string(
    session: &mut SyncSession,
    components: &[u32],
) -> anyhow::Result<Option<String>> {
    let oid = Oid::from(components)
        .map_err(|error| anyhow::anyhow!("OID inválido: {error}"))?;
    let mut response = session
        .get(&oid)
        .map_err(|error| anyhow::anyhow!("SNMP GET: {error}"))?;

    let Some((_name, value)) = response.varbinds.next() else {
        return Ok(None);
    };

    match value {
        Value::OctetString(value) => Ok(Some(String::from_utf8_lossy(value).into_owned())),
        Value::NoSuchInstance | Value::NoSuchObject | Value::EndOfMibView => Ok(None),
        other => Ok(Some(format!("{other:?}"))),
    }
}

fn resolve_mac(target: &str) -> anyhow::Result<String> {
    if parse_mac(target).is_ok() {
        return Ok(target.replace('-', ":").to_ascii_uppercase());
    }

    device::resolve_name_to_mac(target)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no es una MAC válida ni un equipo inventariado: {target}"
        )
    })
}

fn parse_mac(value: &str) -> anyhow::Result<[u8; 6]> {
    let normalized = value.replace('-', ":");
    let parts: Vec<&str> = normalized.split(':').collect();

    if parts.len() != 6 {
        anyhow::bail!("MAC inválida: {value}");
    }

    let mut mac = [0_u8; 6];
    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)
            .map_err(|_| anyhow::anyhow!("MAC inválida: {value}"))?;
    }

    Ok(mac)
}

fn snmp_address(host: &str) -> String {
    if host.parse::<SocketAddr>().is_ok() {
        return host.to_owned();
    }

    if host.contains(':') {
        format!("[{host}]:161")
    } else {
        format!("{host}:161")
    }
}

fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn load() -> anyhow::Result<Vec<SwitchProfile>> {
    let path = database_path();

    if !path.exists() {
        return Ok(Vec::new());
    }

    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn save(switches: &[SwitchProfile]) -> anyhow::Result<()> {
    let path = database_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, serde_json::to_string_pretty(switches)?)?;
    Ok(())
}

fn database_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
        .join("switches.json")
}
