use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;

use crate::model::{
    device::Device,
    network::{NetworkProvider, PresenceRecord},
    switch::SwitchProfile,
};

pub trait DeviceRepository: Send + Sync {
    fn all(&self) -> Result<Vec<Device>>;
    fn replace_all(&self, devices: &[Device]) -> Result<()>;
    fn path(&self) -> PathBuf;

    fn names_by_mac(&self) -> Result<HashMap<String, String>> {
        Ok(self
            .all()?
            .into_iter()
            .map(|device| (device.mac, device.name))
            .collect())
    }

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
    fn path(&self) -> PathBuf;
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
