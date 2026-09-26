use std::{env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::CommandOutput;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Device {
    mac: String,
    name: String,
    notes: Option<String>,
}

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("list");

    match sub {
        "list" => list(),
        "add" => add(&args[1..]),
        "remove" => remove(&args[1..]),
        _ => Ok(CommandOutput::error(
            format!("device: subcomando desconocido: {sub}"),
            2,
        )),
    }
}

fn list() -> anyhow::Result<CommandOutput> {
    let devices = load()?;
    let mut out = String::from("MAC                 NAME\n");

    for device in devices {
        out.push_str(&format!("{:<19} {}\n", device.mac, device.name));
    }

    Ok(CommandOutput::ok(out))
}

fn add(args: &[String]) -> anyhow::Result<CommandOutput> {
    let Some(mac) = args.first() else {
        return Ok(CommandOutput::error(
            "device add: uso: device add MAC NOMBRE",
            2,
        ));
    };

    if args.len() < 2 {
        return Ok(CommandOutput::error("device add: falta nombre", 2));
    }

    let mac = normalize_mac(mac)?;
    let name = args[1..].join(" ");
    let mut devices = load()?;

    if let Some(existing) = devices.iter_mut().find(|device| device.mac == mac) {
        existing.name = name.clone();
    } else {
        devices.push(Device {
            mac: mac.clone(),
            name: name.clone(),
            notes: None,
        });
    }

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
