//! Unix lifecycle evidence with bounded waits and failure cleanup.

use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const POLL: Duration = Duration::from_millis(10);
const REQUESTS: &[u8] = concat!(
    "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"lifecycle_ready\"}}\n",
)
.as_bytes();

#[derive(Debug)]
struct Process {
    pid: u32,
    parent: u32,
    group: u32,
    state: String,
}

fn processes() -> io::Result<Vec<Process>> {
    let output = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,stat="])
        .env_clear()
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("ps failed: {}", output.status)));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let invalid = || io::Error::other("invalid ps identity/state row");
            if fields.len() != 4 {
                return Err(invalid());
            }
            Ok(Process {
                pid: fields[0].parse().map_err(|_| invalid())?,
                parent: fields[1].parse().map_err(|_| invalid())?,
                group: fields[2].parse().map_err(|_| invalid())?,
                state: fields[3].to_owned(),
            })
        })
        .collect()
}

fn signal(target: &str, name: &str) -> io::Result<ExitStatus> {
    Command::new("/bin/kill")
        .args([name, "--", target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

fn pid(identity: &Value, key: &str) -> u32 {
    let value = u32::try_from(identity[key].as_u64().expect("numeric process identity"))
        .expect("process identity fits u32");
    assert!(value > 1, "unsafe process identity: {identity}");
    value
}

struct Gateway {
    child: Child,
    root: TempDir,
    stdout: PathBuf,
    stderr: PathBuf,
    complete: bool,
}

impl Gateway {
    fn spawn() -> Self {
        let root = TempDir::new().unwrap();
        let fake = super::write_fake(&root);
        fs::write(&fake, include_str!("mcp_lifecycle.py")).unwrap();
        let profile = super::write_profile(&root, &fake);
        let stdout = root.path().join("stdout.jsonl");
        let stderr = root.path().join("stderr.log");
        // The shared command clears inherited environment and isolates all XDG
        // paths. Regular capture files cannot hang waiting for an orphan's EOF.
        let child = super::command(&root, &["mcp", "--profile-file", profile.to_str().unwrap()])
            .process_group(0)
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        Self {
            child,
            root,
            stdout,
            stderr,
            complete: false,
        }
    }

    fn ready(&mut self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        self.child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(REQUESTS)
            .unwrap();
        loop {
            let status = self.child.try_wait().unwrap();
            assert!(
                status.is_none(),
                "gateway exited before readiness: {status:?}"
            );
            let output = fs::read_to_string(&self.stdout).unwrap();
            // Parse only complete frames; file existence or a partial PID is
            // never readiness. A routed response proves handlers are installed.
            let responses = output
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .map(|line| serde_json::from_str::<Value>(line).expect("JSON-RPC stdout"))
                .collect::<Vec<_>>();
            assert!(responses.len() <= 2, "unexpected stdout: {output}");
            if responses.len() == 2 {
                assert_eq!(responses[0]["id"], 1);
                assert_eq!(responses[0]["result"]["serverInfo"]["name"], "symbrain");
                assert_eq!(responses[1]["id"], 2);
                assert_eq!(responses[1]["result"]["isError"], false);
                return serde_json::from_str(
                    responses[1]["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "fake child/gateway never became MCP-ready"
            );
            thread::sleep(POLL);
        }
    }

    fn wait(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("wait for gateway") {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(POLL);
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        eprintln!(
            "lifecycle failure stdout: {}\nstderr: {}",
            fs::read_to_string(&self.stdout).unwrap_or_default(),
            fs::read_to_string(&self.stderr).unwrap_or_default()
        );
        // Capture owned groups while the gateway is still their parent. Atomic
        // fixture identity also covers a gateway that already exited/reparented.
        let mut groups = vec![self.child.id()];
        if let Ok(rows) = processes() {
            eprintln!(
                "gateway/child snapshot: {:?}",
                rows.iter()
                    .filter(|p| p.pid == self.child.id() || p.parent == self.child.id())
                    .collect::<Vec<_>>()
            );
            groups.extend(
                rows.iter()
                    .filter(|p| p.parent == self.child.id() && p.group == p.pid)
                    .map(|p| p.group),
            );
        }
        if let Ok(identity) = fs::read(self.root.path().join("identity.json"))
            && let Ok(identity) = serde_json::from_slice::<Value>(&identity)
            && let Some(group) = identity["pgid"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
            && identity["pid"].as_u64() == Some(u64::from(group))
            && group > 1
        {
            eprintln!("fixture identity: {identity}");
            groups.push(group);
        }
        groups.sort_unstable();
        groups.dedup();
        for group in &groups {
            let _ = signal(&format!("-{group}"), "-TERM");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let _ = self.child.try_wait();
            if processes().is_ok_and(|rows| rows.iter().all(|p| !groups.contains(&p.group))) {
                return;
            }
            thread::sleep(POLL);
        }
        for group in &groups {
            let _ = signal(&format!("-{group}"), "-KILL");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let _ = self.child.try_wait();
            if processes().is_ok_and(|rows| rows.iter().all(|p| !groups.contains(&p.group))) {
                return;
            }
            thread::sleep(POLL);
        }
        eprintln!("process groups {groups:?} remained after bounded failure cleanup");
    }
}

pub(super) fn assert_sigterm_shutdown() {
    eprintln!(
        "manifest: {}/Cargo.toml; gateway: {}",
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_BIN_EXE_symbrain")
    );
    let mut gateway = Gateway::spawn();
    let identity = gateway.ready();
    let leader = pid(&identity, "pid");
    let descendant = pid(&identity, "descendant");
    let group = pid(&identity, "pgid");
    assert_eq!(group, leader, "managed child must lead its own group");
    assert_ne!(leader, descendant);
    assert_ne!(group, gateway.child.id());
    let rows = processes().expect("successful process probe before SIGTERM");
    for (id, parent) in [(leader, gateway.child.id()), (descendant, leader)] {
        let process = rows
            .iter()
            .find(|p| p.pid == id)
            .expect("fixture process actually started");
        assert_eq!(process.parent, parent, "{process:?}");
        assert_eq!(process.group, group, "{process:?}");
        assert!(
            !process.state.starts_with('Z'),
            "fixture already a zombie: {process:?}"
        );
    }
    eprintln!("MCP-ready leader={leader}, descendant={descendant}, pgid={group}");
    // Keep gateway stdin open: EOF must not trigger the shutdown under test.
    assert!(
        signal(&gateway.child.id().to_string(), "-TERM")
            .unwrap()
            .success()
    );
    let status = gateway
        .wait(Duration::from_secs(5))
        .expect("native mcp did not stop after SIGTERM");
    assert!(status.success(), "gateway status: {status}");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = processes()
            .expect("successful process probe after SIGTERM")
            .into_iter()
            .filter(|p| p.group == group || p.pid == leader || p.pid == descendant)
            .collect::<Vec<_>>();
        if remaining.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "processes survived shutdown (including zombies): {remaining:?}"
        );
        thread::sleep(POLL);
    }
    let reaped: Value = serde_json::from_slice(
        &fs::read(gateway.root.path().join("reaped.json"))
            .expect("descendant reaped by fake parent"),
    )
    .unwrap();
    assert_eq!(reaped["pid"], descendant);
    assert_eq!(reaped["signaled"], true);
    assert_eq!(reaped["signal"], signal_hook::consts::SIGTERM);
    let output = fs::read_to_string(&gateway.stdout).unwrap();
    assert_eq!(output.lines().count(), 2, "stdout pollution: {output}");
    assert!(
        fs::read(&gateway.stderr).unwrap().is_empty(),
        "unexpected stderr"
    );
    gateway.complete = true;
}
