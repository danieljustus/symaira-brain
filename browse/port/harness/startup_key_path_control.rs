// Executable regression control corrupts only the public owned provider path.
// Compiled with plain rustc by fixture_programs.py (std only, no Cargo).
use std::env;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::process::{exit, Command};

/// Every non-ASCII character and every undecodable byte becomes U+FFFD, so the
/// corrupted path names a directory that does not exist.
fn corrupt(path: &OsString) -> String {
    let mut out = String::new();
    for chunk in path.as_bytes().utf8_chunks() {
        for character in chunk.valid().chars() {
            out.push(if (character as u32) > 127 { '\u{fffd}' } else { character });
        }
        for _ in chunk.invalid() {
            out.push('\u{fffd}');
        }
    }
    out
}

fn main() {
    let arguments: Vec<OsString> = env::args_os().collect();
    let probe = env::var_os("SYMBROWSE_STARTUP_PATH_PROBE").unwrap_or_default();
    if arguments.len() != 3 || probe.is_empty() {
        exit(99);
    }
    match Command::new(probe).arg(corrupt(&arguments[1])).arg(&arguments[2]).status() {
        Ok(status) => exit(status.code().unwrap_or(-1)),
        Err(_) => exit(99),
    }
}
