use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::device::{normalize_mac, parse_mac},
    core::{CommandOutput, ports::{DeviceRepository, WakeOnLanSender}},
};

pub struct WakeOnLanService {
    devices: Arc<dyn DeviceRepository>,
    sender: Arc<dyn WakeOnLanSender>,
}

impl WakeOnLanService {
    pub fn new(devices: Arc<dyn DeviceRepository>, sender: Arc<dyn WakeOnLanSender>) -> Self {
        Self { devices, sender }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error(
                "wol: uso: wol AA:BB:CC:DD:EE:FF|NOMBRE [BROADCAST]",
                2,
            ));
        };

        let mac_text = match normalize_mac(target) {
            Ok(mac) => mac,
            Err(_) => self
                .devices
                .resolve_name_to_mac(target)?
                .ok_or_else(|| anyhow::anyhow!(
                    "wol: no es una MAC válida ni un equipo inventariado: {target}"
                ))?,
        };

        let mac = parse_mac(&mac_text)?;
        let broadcast = args.get(1).map(String::as_str).unwrap_or("255.255.255.255:9");
        self.sender.send(mac, broadcast)?;

        Ok(CommandOutput::ok(format!(
            "magic packet sent to {} via {}\n",
            mac_text, broadcast
        )))
    }
}
