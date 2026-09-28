use std::{env, fs, path::PathBuf};

use anyhow::Result;

const DEFAULT_TERMINAL_CONFIG: &str = r##"# Shell Shock Tool native terminal appearance
# Se aplica al iniciar una nueva ventana de SST.

[appearance]
# acrylic = blur fuerte con tinte; blur = blur simple; glass = DWM clásico.
backdrop = "acrylic"

# 0 = totalmente transparente, 100 = tinte completamente opaco.
background_opacity = 82

# Color del tinte de fondo.
background_color = "#111629"
"##;

const DEFAULT_CONFIG: &str = r#"# Shell Shock Tool portable shell configuration
# Bash-compatible syntax.

alias ll='ls -la'
alias la='ls -a'
alias cls='clear'

# Ejemplos:
# export SST_SITE='laboratorio'
# alias scanlab='net scan 192.168.1.0/24'
"#;

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

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.config_dir())?;
        fs::create_dir_all(self.data_dir())?;

        let config = self.config_file();
        if !config.exists() {
            fs::write(config, DEFAULT_CONFIG)?;
        }

        let terminal = self.terminal_config_file();
        if !terminal.exists() {
            fs::write(terminal, DEFAULT_TERMINAL_CONFIG)?;
        }

        Ok(())
    }

    pub fn data_dir(&self) -> PathBuf { self.root.join("data") }
    pub fn config_dir(&self) -> PathBuf { self.root.join("config") }
    pub fn config_file(&self) -> PathBuf { self.config_dir().join("sstrc") }
    pub fn terminal_config_file(&self) -> PathBuf { self.config_dir().join("terminal.toml") }
    pub fn devices_file(&self) -> PathBuf { self.data_dir().join("devices.json") }
    pub fn presence_file(&self) -> PathBuf { self.data_dir().join("network_presence.json") }
    pub fn providers_file(&self) -> PathBuf { self.data_dir().join("network_providers.json") }
    pub fn switches_file(&self) -> PathBuf { self.data_dir().join("switches.json") }
    pub fn history_file(&self) -> PathBuf { self.data_dir().join("history") }
}
