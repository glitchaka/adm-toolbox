use std::sync::Arc;

use anyhow::Result;

use crate::{
    core::{CommandContext, CommandOutput, ports::TextEditor},
    presentation::helix_sst::{HELIX_SST_VERSION, HELIX_UPSTREAM_VERSION, HELP},
};

use super::BuiltinCommand;

pub struct EditorBuiltin {
    editor: Arc<dyn TextEditor>,
}

impl EditorBuiltin {
    pub fn new(editor: Arc<dyn TextEditor>) -> Self {
        Self { editor }
    }
}

impl BuiltinCommand for EditorBuiltin {
    fn name(&self) -> &'static str {
        "helix"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["hx", "helix-sst"]
    }

    fn help(&self) -> &'static str {
        HELP
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        if args.first().is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h" | "--guide")) {
            return Ok(CommandOutput::ok(HELP));
        }
        if args.iter().any(|arg| matches!(arg.as_str(), "--version" | "-V")) {
            return Ok(CommandOutput::ok(format!(
                "helix-sst {HELIX_SST_VERSION}\nBased on Helix {HELIX_UPSTREAM_VERSION}\nUpstream: helix-editor/helix\nLicense: MPL-2.0\n"
            )));
        }

        if args.iter().any(|arg| arg == "--credits") {
            return Ok(CommandOutput::ok(format!(
                "helix-sst {HELIX_SST_VERSION}\nBased on Helix {HELIX_UPSTREAM_VERSION}\nHelix is developed by the Helix contributors.\nUpstream: https://github.com/helix-editor/helix\nLicense: Mozilla Public License 2.0 (MPL-2.0)\n"
            )));
        }

        let status = self.editor.edit(args, context.cwd)?;
        let stderr = match status {
            0 => String::new(),
            130 => "helix-sst: editor abortado; los cambios sin guardar se descartaron.\n".to_owned(),
            _ => format!("helix-sst terminó con código {status}. Revisa config/helix-sst/helix.log; ayuda: help helix.\n"),
        };
        Ok(CommandOutput { status, stdout: String::new(), stderr })
    }
}
