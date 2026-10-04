//! Empty discovery fixtures need existing parents on Windows: a missing file
//! returns error2 (Go skips it), while a missing parent returns error3 (a real
//! Doctor discovery issue). Keep production's distinction intact.
use std::{fs, path::Path};

pub fn create_parents(home: &Path, xdg_config: &Path) {
    for parent in [
        home.join(".config/hermes"),
        home.join(".cursor"),
        home.join(".vscode"),
        home.join(".config/opencode"),
        xdg_config.join("claude"),
    ] {
        fs::create_dir_all(parent).unwrap();
    }
}
