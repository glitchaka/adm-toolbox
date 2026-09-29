mod admin;
mod command;
mod config;
mod editor;
mod registry;
mod nwash;
mod system;
mod sudo;
mod unix;

pub use admin::{
    DeviceBuiltin, DiagnosticsBuiltin, DomainBuiltin, NetworkBuiltin, SwitchBuiltin,
    WakeOnLanBuiltin,
};
pub use command::BuiltinCommand;
pub use config::{ConfigBuiltin, PathBuiltin};
pub use editor::EditorBuiltin;
pub use registry::CommandRegistry;
pub use nwash::{AclBuiltin, EventLogBuiltin, PnpBuiltin, ProcessBuiltin, RegistryBuiltin, ServiceBuiltin};
pub use system::SystemBuiltin;
pub use sudo::SudoBuiltin;
pub use unix::{UNIX_COMMANDS, UnixBuiltin};
