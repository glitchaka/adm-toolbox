use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::{
        device::DeviceService,
        diagnostics::DiagnosticsService,
        domain::DomainService,
        network::NetworkService,
        switch::SwitchService,
        wol::WakeOnLanService,
    },
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

macro_rules! service_builtin {
    ($type:ident, $name:literal, $service:ty, $field:ident, $help:literal) => {
        pub struct $type {
            $field: Arc<$service>,
        }

        impl $type {
            pub fn new($field: Arc<$service>) -> Self {
                Self { $field }
            }
        }

        impl BuiltinCommand for $type {
            fn name(&self) -> &'static str {
                $name
            }

            fn help(&self) -> &'static str {
                $help
            }

            fn execute(
                &self,
                _invoked_name: &str,
                args: &[String],
                _context: CommandContext<'_>,
            ) -> Result<CommandOutput> {
                self.$field.execute(args)
            }
        }
    };
}

service_builtin!(
    NetworkBuiltin,
    "net",
    NetworkService,
    service,
    "net — diagnóstico, descubrimiento, presencia y tráfico de red"
);
service_builtin!(
    DeviceBuiltin,
    "device",
    DeviceService,
    service,
    "device — inventario local de equipos por MAC"
);
service_builtin!(
    DomainBuiltin,
    "domain",
    DomainService,
    service,
    "domain — pertenencia a dominio local o remota"
);
service_builtin!(
    SwitchBuiltin,
    "switch",
    SwitchService,
    service,
    "switch — resolución MAC → switch → puerto mediante SNMP"
);
service_builtin!(
    WakeOnLanBuiltin,
    "wol",
    WakeOnLanService,
    service,
    "wol — envía Magic Packet por MAC o nombre inventariado"
);
service_builtin!(
    DiagnosticsBuiltin,
    "diag",
    DiagnosticsService,
    service,
    "diag — diagnósticos compuestos de red, hardware, almacenamiento y dominio"
);
