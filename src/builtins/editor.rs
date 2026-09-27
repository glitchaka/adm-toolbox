use std::sync::Arc;

use anyhow::Result;

use crate::{
    core::{CommandContext, CommandOutput, ports::TextEditor},
    presentation::helix_sst::{HELIX_SST_VERSION, HELIX_UPSTREAM_VERSION},
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
        &["hx"]
    }

    fn help(&self) -> &'static str {
        "helix [FILE...] — helix-sst, integración portable basada en Helix 25.07.1"
    }

    fn execute(
        &self,
        _invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
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

        self.editor.edit(args, context.cwd)?;
        Ok(CommandOutput::ok(""))
    }
}
