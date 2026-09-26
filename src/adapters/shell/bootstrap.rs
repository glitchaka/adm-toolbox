use std::path::Path;

use anyhow::Result;
use brush_core::{
    Shell as BrushEngine,
    SourceInfo,
    extensions::DefaultShellExtensions,
};

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
