use std::{
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use brush_builtins::{BuiltinSet, ShellBuilderExt as _};
use brush_core::{
    ExecutionControlFlow,
    Shell as BrushEngine,
    SourceInfo,
    builtins,
    extensions::DefaultShellExtensions,
};
use rustyline::{
    Editor,
    error::ReadlineError,
    history::DefaultHistory,
};
use tokio::runtime::Runtime;

use crate::{
    adapters::shell::{BrushBuiltinBridge, install_registry},
    builtins::CommandRegistry,
    core::ports::FileSystem,
};

use super::{
    bootstrap,
    completion::ShellHelper,
    prompt,
};

pub struct ShellSession {
    editor: Editor<ShellHelper, DefaultHistory>,
    engine: BrushEngine<DefaultShellExtensions>,
    runtime: Runtime,
    history_file: PathBuf,
    running: bool,
}

impl ShellSession {
    pub fn new(
        registry: Arc<CommandRegistry>,
        file_system: Arc<dyn FileSystem>,
        config_file: PathBuf,
        history_file: PathBuf,
    ) -> Result<Self> {
        bootstrap::ensure_config(file_system.as_ref(), &config_file)?;
        install_registry(Arc::clone(&registry))?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("No se pudo inicializar el runtime de la shell")?;

        let command_names = registry.names();
        let mut engine = runtime.block_on(build_engine(&command_names))?;
        runtime.block_on(bootstrap::install(&mut engine, &config_file))?;

        let mut completion_names = command_names;
        completion_names.extend([
            "help".to_owned(),
            "man".to_owned(),
            "config".to_owned(),
        ]);

        let mut editor = Editor::<ShellHelper, DefaultHistory>::new()?;
        editor.set_helper(Some(ShellHelper::new(completion_names)));
        let _ = editor.load_history(&history_file);

        Ok(Self {
            editor,
            engine,
            runtime,
            history_file,
            running: true,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        println!("{}", prompt::banner());

        while self.running {
            let prompt = prompt::render(self.engine.working_dir());

            match self.editor.readline(&prompt) {
                Ok(line) => {
                    let line = line.trim_end();

                    if line.trim().is_empty() {
                        continue;
                    }

                    let _ = self.editor.add_history_entry(line);
                    self.execute(line);
                }
                Err(ReadlineError::Interrupted) => println!("^C"),
                Err(ReadlineError::Eof) => {
                    println!();
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }

        let _ = self.editor.save_history(&self.history_file);
        Ok(())
    }

    fn execute(&mut self, line: &str) {
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
        });

        match result {
            Ok(result) => {
                if matches!(
                    result.next_control_flow,
                    ExecutionControlFlow::ExitShell
                ) {
                    self.running = false;
                }
            }
            Err(error) => eprintln!("adm: {error}"),
        }
    }
}

async fn build_engine(
    command_names: &[String],
) -> Result<BrushEngine<DefaultShellExtensions>> {
    let registration =
        builtins::simple_builtin::<BrushBuiltinBridge, DefaultShellExtensions>();

    let mut builder = BrushEngine::builder()
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
