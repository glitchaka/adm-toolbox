use std::sync::Arc;

use anyhow::Result;

use crate::{
    model::{CommandOutput, device::Device, ports::DeviceRepository},
    support::csv,
};

pub struct DeviceService {
    repository: Arc<dyn DeviceRepository>,
}

impl DeviceService {
    pub fn new(repository: Arc<dyn DeviceRepository>) -> Self {
        Self { repository }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("list") {
            "list" => self.list(&args[1..]),
            "show" => self.show(&args[1..]),
            "add" => self.add(&args[1..]),
            "remove" => self.remove(&args[1..]),
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.repository.path().display()))),
            other => Ok(CommandOutput::error(
                format!("device: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    pub fn resolve_name_to_mac(&self, name: &str) -> Result<Option<String>> {
        self.repository.resolve_name_to_mac(name)
    }

    fn list(&self, args: &[String]) -> Result<CommandOutput> {
        let devices = self.repository.all()?;

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
                    csv::escape(&device.mac),
                    csv::escape(&device.name),
                    csv::escape(device.notes.as_deref().unwrap_or(""))
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

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(query) = args.first() else {
            return Ok(CommandOutput::error(
                "device show: uso: device show MAC|NOMBRE",
                2,
            ));
        };

        let devices = self.repository.all()?;
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

    fn add(&self, args: &[String]) -> Result<CommandOutput> {
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

        let mut devices = self.repository.all()?;
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
        self.repository.replace_all(&devices)?;

        Ok(CommandOutput::ok(format!("device saved: {mac} {name}\n")))
    }

    fn remove(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(mac) = args.first() else {
            return Ok(CommandOutput::error(
                "device remove: uso: device remove MAC",
                2,
            ));
        };

        let mac = normalize_mac(mac)?;
        let mut devices = self.repository.all()?;
        let before = devices.len();
        devices.retain(|device| device.mac != mac);

        if before == devices.len() {
            return Ok(CommandOutput::error(
                format!("device: no existe {mac}"),
                1,
            ));
        }

        self.repository.replace_all(&devices)?;
        Ok(CommandOutput::ok(format!("device removed: {mac}\n")))
    }
}

pub fn normalize_mac(value: &str) -> Result<String> {
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

pub fn parse_mac(value: &str) -> Result<[u8; 6]> {
    let normalized = normalize_mac(value)?;
    let parts: Vec<&str> = normalized.split(':').collect();
    let mut mac = [0_u8; 6];

    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)?;
    }

    Ok(mac)
}
