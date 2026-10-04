//! Actual loader probe; no CLI fallback, provider, store or child construction.
use std::io;
use symbrain_core::config::resolved;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}
fn main() {
    let config = match resolved::load() {
        Ok(config) => config,
        Err(error) => {
            let _ = error.write("", &mut io::stderr());
            std::process::exit(2);
        }
    };
    let value = serde_json::json!({
        "config_path_hex": hex(&symbrain_core::go_path::os_bytes(resolved::default_path().as_os_str())),
        "default_profile_hex": hex(config.default_profile.as_ref()),
        "audit.enabled": config.audit.enabled,
        "audit.verbose": config.audit.verbose,
        "gateway.identity_injection": config.gateway.identity_injection,
        "updatecheck.enabled": config.updatecheck.enabled,
        "servers.vault.binary_path_hex": hex(config.servers.vault.as_ref()),
        "servers.operate.binary_path_hex": hex(config.servers.operate.as_ref()),
        "servers.scope.binary_path_hex": hex(config.servers.scope.as_ref()),
        "patterns.enabled": config.patterns.enabled,
        "patterns.promotion_threshold": config.patterns.promotion_threshold,
        "modules.browse": config.modules.browse,
        "modules.operate": config.modules.operate,
        "modules.scope": config.modules.scope,
    });
    let result = serde_json::to_writer(io::stdout(), &value);
    if let Err(error) = result {
        eprintln!("encode JSON: {error}");
        std::process::exit(1);
    }
    use std::io::Write;
    if let Err(error) = io::stdout().write_all(b"\n") {
        eprintln!("encode JSON: {error}");
        std::process::exit(1);
    }
}
