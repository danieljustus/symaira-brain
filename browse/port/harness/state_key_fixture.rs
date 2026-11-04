// Owned fixture providers only: never contact a real vault or OS keychain.
// Installed as symvault and security; behavior is selected by environment.
// Compiled with plain rustc by fixture_programs.py (std only, no Cargo).
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{exit, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

/// Go encoding/json string escaping, which the ledger readers were written for.
fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() {
    let arguments: Vec<String> = env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let invoked = arguments[0].clone();
    let base = Path::new(&invoked)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = base.strip_suffix(".exe").unwrap_or(&base).to_owned();
    let mut mode = env::var("SYMBROWSE_KEY_PROBE_MODE").unwrap_or_default();
    if name == "security" {
        mode = env::var("SYMBROWSE_KEYCHAIN_PROBE_MODE").unwrap_or_default();
        if mode.is_empty() {
            mode = "44".to_owned();
        }
    } else if mode.is_empty() {
        mode = "3".to_owned();
    }
    if let Some(ledger) = env::var_os("SYMBROWSE_KEY_PROBE_LEDGER").filter(|value| !value.is_empty()) {
        let listed: Vec<String> = arguments[1..].iter().map(|argument| json_string(argument)).collect();
        let row = format!(
            "{{\"arguments\":[{}],\"name\":{},\"pid\":{}}}\n",
            listed.join(","),
            json_string(&name),
            std::process::id()
        );
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(ledger)
            .expect("open ledger");
        file.write_all(row.as_bytes()).expect("write ledger");
    }
    if mode == "descendant" || mode == "pipe-holder" {
        Command::new(&invoked)
            .arg("owned-descendant")
            .env("SYMBROWSE_KEY_PROBE_MODE", "timeout")
            .env("SYMBROWSE_KEYCHAIN_PROBE_MODE", "timeout")
            .stdin(Stdio::null())
            .spawn()
            .expect("start owned descendant");
        if mode == "pipe-holder" {
            exit(3);
        }
        sleep(Duration::from_secs(30));
        return;
    }
    match mode.as_str() {
        "ab" | "cd" => {
            let value = mode.repeat(32);
            if name == "security" {
                println!("{value}");
            } else {
                println!("{{\"value\":\"{value}\"}}");
            }
        }
        "invalid" => println!("not-a-key"),
        "empty" => {}
        "timeout" => sleep(Duration::from_secs(30)),
        other => exit(other.parse::<i32>().expect("numeric provider mode")),
    }
}
