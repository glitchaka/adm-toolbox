use std::path::PathBuf;

use anyhow::Result;

use crate::model::{
    device::Device,
    network::{NetworkProvider, PresenceRecord},
    ports::{DeviceRepository, NetworkProviderRepository, PresenceRepository, SwitchRepository},
    switch::SwitchProfile,
};

use super::{AppPaths, json_store::JsonFileStore};

pub struct JsonDeviceRepository { store: JsonFileStore<Device> }
impl JsonDeviceRepository {
    pub fn new(paths: &AppPaths) -> Self { Self { store: JsonFileStore::new(paths.devices_file()) } }
}
impl DeviceRepository for JsonDeviceRepository {
    fn all(&self) -> Result<Vec<Device>> { self.store.load() }
    fn replace_all(&self, values: &[Device]) -> Result<()> { self.store.save(values) }
    fn path(&self) -> PathBuf { self.store.path() }
}

pub struct JsonPresenceRepository { store: JsonFileStore<PresenceRecord> }
impl JsonPresenceRepository {
    pub fn new(paths: &AppPaths) -> Self { Self { store: JsonFileStore::new(paths.presence_file()) } }
}
impl PresenceRepository for JsonPresenceRepository {
    fn all(&self) -> Result<Vec<PresenceRecord>> { self.store.load() }
    fn replace_all(&self, values: &[PresenceRecord]) -> Result<()> { self.store.save(values) }
    fn path(&self) -> PathBuf { self.store.path() }
}

pub struct JsonNetworkProviderRepository { store: JsonFileStore<NetworkProvider> }
impl JsonNetworkProviderRepository {
    pub fn new(paths: &AppPaths) -> Self { Self { store: JsonFileStore::new(paths.providers_file()) } }
}
impl NetworkProviderRepository for JsonNetworkProviderRepository {
    fn all(&self) -> Result<Vec<NetworkProvider>> { self.store.load() }
    fn replace_all(&self, values: &[NetworkProvider]) -> Result<()> { self.store.save(values) }
    fn path(&self) -> PathBuf { self.store.path() }
}

pub struct JsonSwitchRepository { store: JsonFileStore<SwitchProfile> }
impl JsonSwitchRepository {
    pub fn new(paths: &AppPaths) -> Self { Self { store: JsonFileStore::new(paths.switches_file()) } }
}
impl SwitchRepository for JsonSwitchRepository {
    fn all(&self) -> Result<Vec<SwitchProfile>> { self.store.load() }
    fn replace_all(&self, values: &[SwitchProfile]) -> Result<()> { self.store.save(values) }
    fn path(&self) -> PathBuf { self.store.path() }
}
