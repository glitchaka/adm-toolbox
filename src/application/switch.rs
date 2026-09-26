use std::sync::Arc;

use anyhow::Result;

use crate::{
    core::{
        CommandOutput,
        ports::{DeviceRepository, SwitchLocator, SwitchRepository},
        models::switch::SwitchProfile,
    },
    application::device::{normalize_mac, parse_mac},
    support::options,
};

pub struct SwitchService {
    switches: Arc<dyn SwitchRepository>,
    devices: Arc<dyn DeviceRepository>,
    locator: Arc<dyn SwitchLocator>,
}

impl SwitchService {
    pub fn new(
        switches: Arc<dyn SwitchRepository>,
        devices: Arc<dyn DeviceRepository>,
        locator: Arc<dyn SwitchLocator>,
    ) -> Self {
        Self { switches, devices, locator }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("list") {
            "list" => self.list(args.get(1..).unwrap_or_default()),
            "add" => self.add(args.get(1..).unwrap_or_default()),
            "remove" => self.remove(args.get(1..).unwrap_or_default()),
            "show" => self.show(args.get(1..).unwrap_or_default()),
            "path" => Ok(CommandOutput::ok(format!("{}\n", self.switches.path().display()))),
            "capabilities" => Ok(CommandOutput::ok(self.locator.capabilities())),
            "locate" => self.locate(args.get(1..).unwrap_or_default()),
            other => Ok(CommandOutput::error(
                format!("switch: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    fn list(&self, args: &[String]) -> Result<CommandOutput> {
        let switches = self.switches.all()?;

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

    fn add(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error(
                "switch add: uso: switch add NOMBRE --host HOST --community-env VARIABLE",
                2,
            ));
        };

        let Some(host) = options::value(args, "--host") else {
            return Ok(CommandOutput::error("switch add: falta --host", 2));
        };
        let Some(community_env) = options::value(args, "--community-env") else {
            return Ok(CommandOutput::error(
                "switch add: falta --community-env; la comunidad SNMP no se guarda en texto plano",
                2,
            ));
        };

        let description = options::value(args, "--description").map(str::to_owned);
        let mut switches = self.switches.all()?;

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
        self.switches.replace_all(&switches)?;

        Ok(CommandOutput::ok(format!("switch saved: {} {}\n", name, host)))
    }

    fn remove(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error("switch remove: falta nombre", 2));
        };

        let mut switches = self.switches.all()?;
        let before = switches.len();
        switches.retain(|switch| switch.name != *name);

        if switches.len() == before {
            return Ok(CommandOutput::error(format!("switch: no existe {name}"), 1));
        }

        self.switches.replace_all(&switches)?;
        Ok(CommandOutput::ok(format!("switch removed: {name}\n")))
    }

    fn show(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(name) = args.first() else {
            return Ok(CommandOutput::error("switch show: falta nombre", 2));
        };

        let switches = self.switches.all()?;
        let Some(switch) = switches.iter().find(|switch| switch.name == *name) else {
            return Ok(CommandOutput::error(format!("switch: no existe {name}"), 1));
        };

        Ok(CommandOutput::ok(format!(
            "name: {}\nhost: {}\ncommunity_env: {}\ndescription: {}\n",
            switch.name,
            switch.host,
            switch.community_env,
            switch.description.as_deref().unwrap_or("-")
        )))
    }

    fn locate(&self, args: &[String]) -> Result<CommandOutput> {
        let Some(target) = args.first() else {
            return Ok(CommandOutput::error(
                "switch locate: uso: switch locate MAC|NOMBRE [--switch NOMBRE] [--vlan N]",
                2,
            ));
        };

        let mac_text = match normalize_mac(target) {
            Ok(mac) => mac,
            Err(_) => self
                .devices
                .resolve_name_to_mac(target)?
                .ok_or_else(|| anyhow::anyhow!(
                    "no es una MAC válida ni un equipo inventariado: {target}"
                ))?,
        };
        let mac = parse_mac(&mac_text)?;
        let selected_switch = options::value(args, "--switch");
        let vlan = options::value(args, "--vlan").and_then(|value| value.parse::<u32>().ok());
        let json = options::has(args, "--json");

        let switches = self.switches.all()?;
        if switches.is_empty() {
            return Ok(CommandOutput::error(
                "switch locate: no hay switches configurados; usa 'switch add'",
                2,
            ));
        }

        let candidates: Vec<_> = switches
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
            match self.locator.locate(switch, mac, &mac_text, vlan) {
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
                format!("switch locate: MAC no encontrada; consultas con error:\n{}", errors.join("\n")),
                1,
            ));
        }

        Ok(CommandOutput::error(
            format!("switch locate: {} no aparece en los switches consultados", mac_text),
            1,
        ))
    }
}
