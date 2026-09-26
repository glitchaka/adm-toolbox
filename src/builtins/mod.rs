mod admin;
mod command;
mod config;
mod editor;
mod registry;
mod system;
mod unix;

pub use admin::{
    DeviceBuiltin, DiagnosticsBuiltin, DomainBuiltin, NetworkBuiltin, SwitchBuiltin,
    WakeOnLanBuiltin,
};
pub use command::BuiltinCommand;
pub use config::ConfigBuiltin;
pub use editor::EditorBuiltin;
pub use registry::CommandRegistry;
pub use system::SystemBuiltin;
pub use unix::UnixBuiltin;
