// Actual child-process controls alter only explicitly public fixture inputs.
// Compiled with plain rustc by fixture_programs.py (std only, no Cargo).
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{exit, Command};

fn main() {
    let arguments: Vec<OsString> = env::args_os().collect();
    match env::var("SYMBROWSE_STARTUP_OWNER_CONTROL").unwrap_or_default().as_str() {
        "malformed" => {
            eprint!(r#"{{"result":"Exited","status":0,"unexpected":"field"}}"#);
            return;
        }
        "unavailable" => {
            let security = arguments.len() > 2
                && Path::new(&arguments[2]).file_name().is_some_and(|name| name == "security");
            if security {
                // Darwin's owned absence fixture returns actual security exit44.
                // This mutant lies only about the denied vault, not the keychain.
                eprint!(r#"{{"result":"Exited","status":11264}}"#);
            } else {
                eprint!(r#"{{"result":"Unavailable"}}"#);
            }
            return;
        }
        _ => {}
    }
    if arguments.len() < 4 {
        exit(99);
    }
    let mut child = Command::new(&arguments[2]);
    child.args(&arguments[3..]);
    match arguments[1].to_str() {
        Some("wrong-environment") => child.env("SYMBROWSE_ENCRYPTION_KEY", "cd".repeat(32)),
        Some("wrong-vault") => child.env("SYMBROWSE_KEY_PROBE_MODE", "cd"),
        Some("denied-to-absence") => child.env("SYMBROWSE_KEY_PROBE_MODE", "2"),
        _ => exit(99),
    };
    match child.status() {
        // A signal-terminated child has no code; Go's ExitCode reports -1.
        Ok(status) => exit(status.code().unwrap_or(-1)),
        Err(_) => exit(99),
    }
}
