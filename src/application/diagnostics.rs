use std::sync::Arc;

use anyhow::Result;

use crate::core::CommandOutput;

use super::{
    domain::DomainService,
    network::NetworkService,
    system::SystemService,
};

pub struct DiagnosticsService {
    network: Arc<NetworkService>,
    system: Arc<SystemService>,
    domain: Arc<DomainService>,
}

impl DiagnosticsService {
    pub fn new(
        network: Arc<NetworkService>,
        system: Arc<SystemService>,
        domain: Arc<DomainService>,
    ) -> Self {
        Self {
            network,
            system,
            domain,
        }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        match args.first().map(String::as_str).unwrap_or("network") {
            "network" => self.network.diagnose(),
            "dns" => {
                let target = args.get(1).cloned().unwrap_or_else(|| "example.com".to_owned());
                self.network.execute(&["dns".to_owned(), target])
            }
            "hardware" => {
                let mut out = String::from("== system ==\n");
                out.push_str(&self.system.execute(&["info".to_owned()])?.stdout);
                out.push_str("\n== memory ==\n");
                out.push_str(&self.system.execute(&["memory".to_owned()])?.stdout);
                out.push_str("\n== disks ==\n");
                out.push_str(&self.system.execute(&["disks".to_owned()])?.stdout);
                Ok(CommandOutput::ok(out))
            }
            "storage" => self.system.execute(&["disks".to_owned()]),
            "traffic" => self.network.execute(&["traffic".to_owned()]),
            "domain" => self.domain.execute(&["status".to_owned()]),
            other => Ok(CommandOutput::error(
                format!("diag: tema no implementado: {other}"),
                2,
            )),
        }
    }
}
