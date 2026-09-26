use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::core::{
    ShellExecution,
    models::{
        device::Device,
        domain::DomainStatus,
        network::{ByteCounters, ConnectionRow, EchoReply, NetworkProvider, PresenceRecord},
        switch::{LocatedPort, SwitchProfile},
    },
};

pub trait NetworkProbe: Send + Sync {
    fn interfaces(&self) -> Result<String>;
    fn connections(&self) -> Result<Vec<ConnectionRow>>;
    fn routes(&self) -> Result<String>;
    fn neighbors(&self) -> Result<HashMap<std::net::Ipv4Addr, String>>;
    fn echo(&self, address: std::net::Ipv4Addr, ttl: u8, timeout: std::time::Duration) -> Result<EchoReply>;
}

pub trait TextEditor: Send + Sync {
    fn edit(&self, path: &Path) -> Result<()>;
}

pub trait ShellEngine: Send {
    fn working_dir(&self) -> &Path;
    fn execute(&mut self, line: &str) -> Result<ShellExecution>;
    fn set_arguments(&mut self, name: &str, args: &[String]);
}

pub trait DeviceRepository: Send + Sync {
    fn all(&self) -> Result<Vec<Device>>;
    fn replace_all(&self, devices: &[Device]) -> Result<()>;
    fn path(&self) -> PathBuf;

    fn resolve_name_to_mac(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .all()?
            .into_iter()
            .find(|device| device.name.eq_ignore_ascii_case(name))
            .map(|device| device.mac))
    }
}

pub trait PresenceRepository: Send + Sync {
    fn all(&self) -> Result<Vec<PresenceRecord>>;
    fn replace_all(&self, records: &[PresenceRecord]) -> Result<()>;
}

pub trait NetworkProviderRepository: Send + Sync {
    fn all(&self) -> Result<Vec<NetworkProvider>>;
    fn replace_all(&self, providers: &[NetworkProvider]) -> Result<()>;
    fn path(&self) -> PathBuf;
}

pub trait SwitchRepository: Send + Sync {
    fn all(&self) -> Result<Vec<SwitchProfile>>;
    fn replace_all(&self, switches: &[SwitchProfile]) -> Result<()>;
    fn path(&self) -> PathBuf;
}

pub trait DomainProbe: Send + Sync {
    fn local_status(&self) -> Result<DomainStatus>;
    fn remote_status(&self, host: &str, verify: bool) -> Result<DomainStatus>;
}

pub trait SwitchLocator: Send + Sync {
    fn locate(
        &self,
        profile: &SwitchProfile,
        mac: [u8; 6],
        mac_text: &str,
        vlan: Option<u32>,
    ) -> Result<Option<LocatedPort>>;

    fn capabilities(&self) -> &'static str;
}

pub trait WakeOnLanSender: Send + Sync {
    fn send(&self, mac: [u8; 6], broadcast: &str) -> Result<()>;
}

pub trait ForegroundProcessProvider: Send + Sync {
    fn foreground_pid(&self) -> Option<u32>;
}

pub trait TrafficMonitor: Send {
    fn snapshot(&self) -> HashMap<u32, ByteCounters>;
}

pub trait TrafficMonitorFactory: Send + Sync {
    fn start(&self) -> Result<Box<dyn TrafficMonitor>>;
    fn rates(
        &self,
        before: &HashMap<u32, ByteCounters>,
        after: &HashMap<u32, ByteCounters>,
        elapsed: std::time::Duration,
    ) -> HashMap<u32, ByteCounters>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    Char(char),
    Escape,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Backspace,
    Delete,
    Space,
    Other,
}

pub trait TerminalSession: Send {
    fn size(&self) -> Result<(u16, u16)>;
    fn clear(&mut self) -> Result<()>;
    fn write(&mut self, text: &str) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
    fn poll_key(&mut self, timeout: std::time::Duration) -> Result<Option<TerminalKey>>;
}

pub trait TerminalFactory: Send + Sync {
    fn alternate_screen(&self) -> Result<Box<dyn TerminalSession>>;
}
