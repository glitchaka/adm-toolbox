mod diagnostics;
mod discovery;
mod provider;
mod traffic;

use std::sync::Arc;

use anyhow::Result;

use crate::core::CommandOutput;

pub use diagnostics::NetworkDiagnosticsService;
pub use discovery::NetworkDiscoveryService;
pub use provider::NetworkProviderService;
pub use traffic::NetworkTrafficService;

pub struct NetworkService {
    diagnostics: Arc<NetworkDiagnosticsService>,
    discovery: Arc<NetworkDiscoveryService>,
    traffic: Arc<NetworkTrafficService>,
    providers: Arc<NetworkProviderService>,
}

impl NetworkService {
    pub fn new(
        diagnostics: Arc<NetworkDiagnosticsService>,
        discovery: Arc<NetworkDiscoveryService>,
        traffic: Arc<NetworkTrafficService>,
        providers: Arc<NetworkProviderService>,
    ) -> Self {
        Self {
            diagnostics,
            discovery,
            traffic,
            providers,
        }
    }

    pub fn execute(&self, args: &[String]) -> Result<CommandOutput> {
        let sub = args.first().map(String::as_str).unwrap_or("interfaces");

        match sub {
            "interfaces" => self.diagnostics.interfaces(),
            "connections" => self.diagnostics.connections(),
            "routes" => self.diagnostics.routes(),
            "dns" => self.diagnostics.dns(&args[1..]),
            "ping" => self.diagnostics.ping(&args[1..]),
            "trace" | "traceroute" => self.diagnostics.trace(&args[1..]),
            "neighbors" | "arp" => self.diagnostics.arp_table(),
            "ports" => self.diagnostics.ports(&args[1..]),
            "scan" => self.discovery.scan(&args[1..]),
            "monitor" => self.discovery.monitor(&args[1..]),
            "presence" => self.discovery.presence(&args[1..]),
            "traffic" => self.traffic.execute(&args[1..]),
            "usage" => self.providers.usage(&args[1..]),
            "provider" => self.providers.execute(&args[1..]),
            other => Ok(CommandOutput::error(
                format!("net: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    pub fn diagnose(&self) -> Result<CommandOutput> {
        self.diagnostics.diagnose()
    }

    pub fn traffic_service(&self) -> &NetworkTrafficService {
        &self.traffic
    }
}
