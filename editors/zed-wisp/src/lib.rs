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
        // Try to find wisp_driver in PATH first
        let path = worktree
            .which("wisp_driver")
            .unwrap_or_else(|| "/Users/aaronmahlke/git/wisp/compiler/target/release/wisp_driver".to_string());

        Ok(zed::Command {
            command: path,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(WispExtension);

