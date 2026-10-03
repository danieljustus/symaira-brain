//! Compiled cross-platform vault fixture; stores only synthetic test input.
use std::{env, fs::OpenOptions, io::{Read, Write}};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn argument_bytes(arg: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)] { use std::os::unix::ffi::OsStrExt; arg.as_bytes().to_vec() }
    #[cfg(not(unix))] { arg.to_string_lossy().as_bytes().to_vec() }
}
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).unwrap();
    if let Some(path) = env::var_os("FAKE_VAULT_LOG") {
        let mut file = OpenOptions::new().create(true).append(true).open(path).unwrap();
        writeln!(file,"argshex={};stdinhex={}",args.iter().map(|arg| hex(&argument_bytes(arg))).collect::<Vec<_>>().join(" "),hex(&input)).unwrap();
    }
    let code = if env::var_os("FAKE_VAULT_PASSTHROUGH").is_some() {
        std::io::stdout().write_all(&input).unwrap();
        println!("opaque child");
        eprintln!("opaque stderr");
        env::var("FAKE_VAULT_ACTION_EXIT").unwrap_or_else(|_| "0".to_owned())
    } else if args.first().is_some_and(|arg| arg == "get") {
        print!("{}",env::var("FAKE_VAULT_METADATA").unwrap_or_else(|_| "{}".to_owned()));
        eprintln!("fixture-secret must not leak");
        env::var("FAKE_VAULT_GET_EXIT").unwrap_or_else(|_| "0".to_owned())
    } else {
        println!("fixture-secret must not leak");
        eprintln!("fixture-secret must not leak");
        env::var("FAKE_VAULT_ACTION_EXIT").unwrap_or_else(|_| "0".to_owned())
    };
    std::process::exit(code.parse().unwrap());
}
