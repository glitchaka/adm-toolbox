use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use brush_builtins::{BuiltinSet, ShellBuilderExt as _};
use brush_core::{
    ExecutionControlFlow,
    Shell as BrushShell,
    SourceInfo,
    builtins,
    extensions::DefaultShellExtensions,
};
use tokio::runtime::Runtime;

use crate::{
    builtins::CommandRegistry,
    core::{
        ShellExecution,
        ports::ShellEngine,
    },
};

use super::{
    bootstrap,
    brush_bridge::{BrushBuiltinBridge, install_registry},
};

pub struct BrushShellEngine {
    engine: BrushShell<DefaultShellExtensions>,
    runtime: Runtime,
}

impl BrushShellEngine {
    pub fn new(
        registry: Arc<CommandRegistry>,
        command_names: &[String],
        config_file: PathBuf,
    ) -> Result<Self> {
        install_registry(registry)?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("No se pudo inicializar el runtime de la shell")?;

        let mut engine = runtime.block_on(build_engine(command_names))?;
        runtime.block_on(bootstrap::install(&mut engine, &config_file))?;

        Ok(Self { engine, runtime })
    }
}

impl ShellEngine for BrushShellEngine {
    fn working_dir(&self) -> &Path {
        self.engine.working_dir()
    }

    fn execute(&mut self, line: &str) -> Result<ShellExecution> {
        let history_command = format!("history -s -- {}", bootstrap::quote(line));

        let result = self.runtime.block_on(async {
            let history_params = self.engine.default_exec_params();
            let _ = self
                .engine
                .run_string(
                    &history_command,
                    &SourceInfo::default(),
                    &history_params,
                )
                .await;

            let params = self.engine.default_exec_params();
            self.engine
                .run_string(line, &SourceInfo::default(), &params)
                .await
        })?;

        if matches!(
            result.next_control_flow,
            ExecutionControlFlow::ExitShell
        ) {
            Ok(ShellExecution::exit())
        } else {
            Ok(ShellExecution::continue_running())
        }
    }
}

async fn build_engine(
    command_names: &[String],
) -> Result<BrushShell<DefaultShellExtensions>> {
    let registration =
        builtins::simple_builtin::<BrushBuiltinBridge, DefaultShellExtensions>();

    let mut builder = BrushShell::builder()
        .interactive(true)
        .shell_name("adm-toolbox".to_owned())
        .shell_product_display_str("ADM Toolbox · Rust Bash engine".to_owned())
        .default_builtins(BuiltinSet::BashMode);

    for name in command_names {
        builder = builder.builtin(name.clone(), registration.clone());
    }

    for name in ["help", "man"] {
        builder = builder.builtin(name.to_owned(), registration.clone());
    }

    Ok(builder.build().await?)
}
