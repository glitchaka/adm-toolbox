use std::path::Path;

use anyhow::{Context, Result};
use brush_core::{
    Shell as BrushEngine,
    SourceInfo,
    extensions::DefaultShellExtensions,
};

use crate::core::ports::FileSystem;

const DEFAULT_CONFIG: &str = r#"# ADM Toolbox portable shell configuration
# Bash-compatible syntax.

alias ll='ls -la'
alias la='ls -a'
alias cls='clear'

# Ejemplos:
# export ADM_SITE='laboratorio'
# alias scanlab='net scan 192.168.1.0/24'
"#;

pub fn ensure_config(file_system: &dyn FileSystem, config_file: &Path) -> Result<()> {
    if file_system.exists(config_file) {
        return Ok(());
    }

    file_system
        .write(config_file, DEFAULT_CONFIG.as_bytes())
        .with_context(|| format!("No se pudo crear {}", config_file.display()))
}

pub async fn install(
    engine: &mut BrushEngine<DefaultShellExtensions>,
    config_file: &Path,
) -> Result<()> {
    let quoted_config = bash_quote(&config_file.to_string_lossy());

    let script = format!(
        r#"
export ADM_CONFIG={quoted_config}

config() {{
    case "$1" in
        reload)
            if [ -f "$ADM_CONFIG" ]; then
                . "$ADM_CONFIG"
            fi
            ;;
        *)
            adm-config "$@"
            ;;
    esac
}}

cd() {{
    case "$1" in
        /[A-Za-z]/*)
            builtin cd "$(adm-path "$1")"
            ;;
        *)
            builtin cd "$@"
            ;;
    esac
}}

if [ -f "$ADM_CONFIG" ]; then
    . "$ADM_CONFIG"
fi
"#
    );

    let params = engine.default_exec_params();
    engine
        .run_string(&script, &SourceInfo::default(), &params)
        .await?;

    Ok(())
}

pub fn quote(value: &str) -> String {
    bash_quote(value)
}

fn bash_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
