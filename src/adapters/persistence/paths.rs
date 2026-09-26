use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Self {
        let root = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        Self { root }
    }

    pub fn root(&self) -> &PathBuf { &self.root }
    pub fn data_dir(&self) -> PathBuf { self.root.join("data") }
    pub fn config_dir(&self) -> PathBuf { self.root.join("config") }
    pub fn config_file(&self) -> PathBuf { self.config_dir().join("admrc") }
    pub fn devices_file(&self) -> PathBuf { self.data_dir().join("devices.json") }
    pub fn presence_file(&self) -> PathBuf { self.data_dir().join("network_presence.json") }
    pub fn providers_file(&self) -> PathBuf { self.data_dir().join("network_providers.json") }
    pub fn switches_file(&self) -> PathBuf { self.data_dir().join("switches.json") }
    pub fn history_file(&self) -> PathBuf { self.data_dir().join("history") }
}
