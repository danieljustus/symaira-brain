//! Owned differential fixture; delegates real Git and mutates only archive I/O.
use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};
fn main() {
    if let Err(error) = run() {
        eprintln!("owned Git fixture: {error}");
        std::process::exit(101);
    }
}
fn run() -> std::io::Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let real = std::env::var_os("SKILLS_FIXTURE_REAL_GIT")
        .ok_or_else(|| std::io::Error::other("owned real Git missing"))?;
    let ledger = std::env::var_os("SKILLS_FIXTURE_GIT_LEDGER")
        .ok_or_else(|| std::io::Error::other("owned Git ledger missing"))?;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger)?;
    // Native code units are retained; no lossy argv0 equality is claimed.
    let bytes: Vec<Vec<u8>> = args
        .iter()
        .map(|arg| arg.as_encoded_bytes().to_vec())
        .collect();
    writeln!(
        log,
        "{}",
        serde_json::to_string(&bytes).map_err(std::io::Error::other)?
    )?;
    if args.get(1).is_some_and(|arg| arg == "archive") {
        match std::env::var("SKILLS_FIXTURE_GIT_MODE").as_deref() {
            Ok("warning") => eprintln!("owned archive warning before valid tar"),
            Ok("failure") => {
                eprintln!("owned archive diagnostic failure");
                std::process::exit(37);
            }
            _ => {}
        }
    }
    let status = Command::new(real)
        .args(&args[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}
