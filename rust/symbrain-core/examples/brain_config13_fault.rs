//! Actual negative controls: delegate the candidate, then inject owned effects.
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn inject_http() -> io::Result<()> {
    let url = std::env::var("SYMBRAIN_RELEASE_BASE_URL")
        .map_err(|error| io::Error::other(error.to_string()))?;
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|tail| tail.parse::<u16>().ok())
        .ok_or_else(|| io::Error::other("control requires owned loopback URL"))?;
    let mut socket = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    socket.write_all(format!("GET /config13-injected-admission HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes())?;
    io::copy(&mut socket, &mut io::sink())?;
    Ok(())
}
fn inject_file() -> io::Result<()> {
    let root = std::env::var_os("CONFIG13_CONTROL_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| io::Error::other("missing owned control root"))?;
    let home = std::env::var_os(symbrain_core::go_path::home_variable())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| io::Error::other("missing owned HOME"))?;
    if !root.is_absolute() || !home.starts_with(&root) {
        return Err(io::Error::other("control HOME must be below owned root"));
    }
    std::fs::write(
        home.join("config13-injected-admission"),
        b"forbidden pre-admission mutation",
    )
}
fn main() {
    let Some(binary) = std::env::var_os("CONFIG13_CONTROL_NATIVE") else {
        eprintln!("missing delegated control executable");
        std::process::exit(78);
    };
    let mode = std::env::var("CONFIG13_CONTROL_MODE").unwrap_or_default();
    let mut child = Command::new(binary)
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("owned delegated candidate");
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).expect("owned input");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&input)
        .expect("delegate input");
    let output = child.wait_with_output().expect("delegate completion");
    let mut code = output.status.code().unwrap_or(1);
    let mut stderr = output.stderr;
    let mut stdout = output.stdout;
    if code == 2 {
        let effect = match mode.as_str() {
            "premature-http" => inject_http(),
            "premature-fs" => inject_file(),
            "wrong-exit" => {
                code = 1;
                Ok(())
            }
            "wrong-field-error" => {
                stderr.extend_from_slice(b"injected wrong field\n");
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = effect {
            eprintln!("owned negative control failed: {error}");
            std::process::exit(78);
        }
    } else if code == 0 && mode == "wrong-value" {
        if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&stdout)
            && let Some(flag) = value
                .get("audit.enabled")
                .and_then(serde_json::Value::as_bool)
        {
            value["audit.enabled"] = serde_json::Value::Bool(!flag);
            stdout = serde_json::to_vec(&value).unwrap();
            stdout.push(b'\n');
        }
    }
    io::stdout().write_all(&stdout).expect("control stdout");
    io::stderr().write_all(&stderr).expect("control stderr");
    std::process::exit(code);
}
