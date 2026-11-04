//! Browser-profile CLI adapter, distinct from the MCP tool-group catalog.

use std::{path::PathBuf, process::ExitCode};
use symbrowse_core::{
    output::{Envelope, Format},
    profiles,
};

use super::write_stdout;

pub(super) fn run(format: Format) -> ExitCode {
    // Go profiles.Discover suppresses directory enumeration errors. Do not
    // load browser preferences, initialize a daemon, or consult credentials.
    let found = browser_root()
        .and_then(|root| profiles::discover(&root).ok())
        .unwrap_or_default();
    if format == Format::Text {
        if found.is_empty() {
            return write_stdout("no Chrome profiles found\n");
        }
        let text: String = found
            .iter()
            .map(|profile| {
                format!(
                    "{}\t{}{}\n",
                    profile.name,
                    profile.path.display(),
                    if profile.is_default { " (default)" } else { "" },
                )
            })
            .collect();
        return write_stdout(&text);
    }
    if format == Format::Yaml {
        return write_stdout(&profiles::render_yaml(&found));
    }
    let data = serde_json::json!({"profiles": (!found.is_empty()).then_some(&found)});
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(text) => write_stdout(&text),
        Err(_) => ExitCode::from(1),
    }
}

fn browser_root() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    if home.is_empty() {
        return None;
    }
    Some(if cfg!(target_os = "macos") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("Google")
            .join("Chrome")
            .join("User Data")
    } else {
        PathBuf::from(home).join(".config").join("google-chrome")
    })
}
