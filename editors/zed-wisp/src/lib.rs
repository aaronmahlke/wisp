use zed_extension_api::{self as zed, LanguageServerId, Result};

struct WispExtension;

impl zed::Extension for WispExtension {
    fn new() -> Self {
        WispExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // Find wisp in PATH - it should be installed to ~/.cargo/bin/wisp
        let path = worktree
            .which("wisp")
            .ok_or_else(|| "wisp not found in PATH. Run: cargo install --path compiler/crates/wisp_driver".to_string())?;

        Ok(zed::Command {
            command: path,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(WispExtension);

