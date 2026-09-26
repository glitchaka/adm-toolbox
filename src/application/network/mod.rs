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
            "dns" => self.diagnostics.dns(args.get(1..).unwrap_or_default()),
            "ping" => self.diagnostics.ping(args.get(1..).unwrap_or_default()),
            "trace" | "traceroute" => self.diagnostics.trace(args.get(1..).unwrap_or_default()),
            "neighbors" | "arp" => self.diagnostics.arp_table(),
            "ports" => self.diagnostics.ports(args.get(1..).unwrap_or_default()),
            "scan" => self.discovery.scan(args.get(1..).unwrap_or_default()),
            "monitor" => self.discovery.monitor(args.get(1..).unwrap_or_default()),
            "presence" => self.discovery.presence(args.get(1..).unwrap_or_default()),
            "traffic" => self.traffic.execute(args.get(1..).unwrap_or_default()),
            "usage" => self.providers.usage(args.get(1..).unwrap_or_default()),
            "provider" => self.providers.execute(args.get(1..).unwrap_or_default()),
            other => Ok(CommandOutput::error(
                format!("net: subcomando desconocido: {other}"),
                2,
            )),
        }
    }

    pub fn diagnose(&self) -> Result<CommandOutput> {
        self.diagnostics.diagnose()
    }

}
