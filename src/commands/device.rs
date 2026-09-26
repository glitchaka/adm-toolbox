use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use super::CommandOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub mac: String,
    pub name: String,
    pub notes: Option<String>,
}

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("list");

    match sub {
        "list" => list(&args[1..]),
        "show" => show(&args[1..]),
        "add" => add(&args[1..]),
        "remove" => remove(&args[1..]),
        "path" => Ok(CommandOutput::ok(format!(
            "{}\n",
            database_path().display()
        ))),
        _ => Ok(CommandOutput::error(
            format!("device: subcomando desconocido: {sub}"),
            2,
        )),
    }
}

fn list(args: &[String]) -> anyhow::Result<CommandOutput> {
    let devices = load()?;

    if args.iter().any(|arg| arg == "--json") {
        return Ok(CommandOutput::ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&devices)?
        )));
    }

    if args.iter().any(|arg| arg == "--csv") {
        let mut out = String::from("mac,name,notes\n");
        for device in devices {
            out.push_str(&format!(
                "{},{},{}\n",
                csv_escape(&device.mac),
                csv_escape(&device.name),
                csv_escape(device.notes.as_deref().unwrap_or(""))
            ));
        }
        return Ok(CommandOutput::ok(out));
    }

    let mut out = String::from("MAC                 NAME                           NOTES\n");

    for device in devices {
        out.push_str(&format!(
            "{:<19} {:<30} {}\n",
            device.mac,
            device.name,
            device.notes.unwrap_or_default()
        ));
    }

    Ok(CommandOutput::ok(out))
}

fn show(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(query) = args.first() else {
        return Ok(CommandOutput::error(
            "device show: uso: device show MAC|NOMBRE",
            2,
        ));
    };

    let devices = load()?;
    let normalized = normalize_mac(query).ok();

    let found = devices.iter().find(|device| {
        normalized
            .as_ref()
            .is_some_and(|mac| device.mac.eq_ignore_ascii_case(mac))
            || device.name.eq_ignore_ascii_case(query)
    });

    let Some(device) = found else {
        return Ok(CommandOutput::error(
            format!("device: no se encontró {query}"),
            1,
        ));
    };

    Ok(CommandOutput::ok(format!(
        "name: {}\nmac: {}\nnotes: {}\n",
        device.name,
        device.mac,
        device.notes.as_deref().unwrap_or("-")
    )))
}

fn add(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(mac) = args.first() else {
        return Ok(CommandOutput::error(
            "device add: uso: device add MAC NOMBRE [--note TEXTO]",
            2,
        ));
    };

    if args.len() < 2 {
        return Ok(CommandOutput::error("device add: falta nombre", 2));
    }

    let mac = normalize_mac(mac)?;
    let mut name_parts = Vec::new();
    let mut note = None;
    let mut index = 1;

    while index < args.len() {
        if args[index] == "--note" {
            note = Some(args[index + 1..].join(" "));
            break;
        }

        name_parts.push(args[index].clone());
        index += 1;
    }

    let name = name_parts.join(" ");
    if name.trim().is_empty() {
        return Ok(CommandOutput::error("device add: falta nombre", 2));
    }

    let mut devices = load()?;

    if let Some(existing) = devices.iter_mut().find(|device| device.mac == mac) {
        existing.name = name.clone();
        if note.is_some() {
            existing.notes = note.clone();
        }
    } else {
        devices.push(Device {
            mac: mac.clone(),
            name: name.clone(),
            notes: note,
        });
    }

    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    save(&devices)?;

    Ok(CommandOutput::ok(format!("device saved: {mac} {name}\n")))
}

fn remove(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(mac) = args.first() else {
        return Ok(CommandOutput::error(
            "device remove: uso: device remove MAC",
            2,
        ));
    };

    let mac = normalize_mac(mac)?;
    let mut devices = load()?;
    let before = devices.len();

    devices.retain(|device| device.mac != mac);

    if before == devices.len() {
        return Ok(CommandOutput::error(
            format!("device: no existe {mac}"),
            1,
        ));
    }

    save(&devices)?;
    Ok(CommandOutput::ok(format!("device removed: {mac}\n")))
}

pub fn known_macs() -> anyhow::Result<HashSet<String>> {
    Ok(load()?.into_iter().map(|device| device.mac).collect())
}

pub fn known_names() -> anyhow::Result<HashMap<String, String>> {
    Ok(load()?
        .into_iter()
        .map(|device| (device.mac, device.name))
        .collect())
}

pub fn resolve_name_to_mac(name: &str) -> anyhow::Result<Option<String>> {
    Ok(load()?
        .into_iter()
        .find(|device| device.name.eq_ignore_ascii_case(name))
        .map(|device| device.mac))
}

fn load() -> anyhow::Result<Vec<Device>> {
    let path = database_path();

    if !path.exists() {
        return Ok(Vec::new());
    }

    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn save(devices: &[Device]) -> anyhow::Result<()> {
    let path = database_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, serde_json::to_string_pretty(devices)?)?;
    Ok(())
}

fn database_path() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
        .join("devices.json")
}

fn normalize_mac(value: &str) -> anyhow::Result<String> {
    let normalized = value.replace('-', ":").to_ascii_uppercase();
    let parts: Vec<&str> = normalized.split(':').collect();

    if parts.len() != 6
        || parts
            .iter()
            .any(|part| part.len() != 2 || u8::from_str_radix(part, 16).is_err())
    {
        anyhow::bail!("MAC inválida: {value}");
    }

    Ok(normalized)
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
