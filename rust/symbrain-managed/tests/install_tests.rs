use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use std::thread;
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use symbrain_managed::{Core, Installer, Platform, installed_version, versions_match};

struct TestServer {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start(routes: BTreeMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_thread = Arc::clone(&requests);
        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut request_line = String::new();
                        let mut reader = BufReader::new(&mut stream);
                        let _ = reader.read_line(&mut request_line);
                        loop {
                            let mut header = String::new();
                            if reader.read_line(&mut header).unwrap_or(0) == 0
                                || header == "\r\n"
                                || header == "\n"
                            {
                                break;
                            }
                        }
                        drop(reader);
                        let path = request_line
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        requests_thread.lock().unwrap().push(path.to_string());
                        if let Some(body) = routes.get(path) {
                            write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .unwrap();
                            stream.write_all(body).unwrap();
                        } else {
                            stream
                                .write_all(
                                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                )
                                .unwrap();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn archive(platform: Platform, binary_name: &str, bytes: &[u8]) -> Vec<u8> {
    if platform.os == "windows" {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(binary_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
        return zip.finish().unwrap().into_inner();
    }

    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_path(binary_name).unwrap();
    header.set_mode(0o755);
    header.set_size(bytes.len().try_into().unwrap());
    header.set_cksum();
    tar.append(&header, bytes).unwrap();
    let encoder = tar.into_inner().unwrap();
    encoder.finish().unwrap()
}

fn fixture_binary(platform: Platform, unix_bytes: &'static [u8]) -> &'static [u8] {
    if platform.os == "windows" {
        b"MZ native Windows install fixture\r\n"
    } else {
        unix_bytes
    }
}

#[cfg(windows)]
fn compile_version_probe(temp: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let source_path = temp.join(format!("{name}.rs"));
    let binary_path = temp.join(format!("{name}.exe"));
    std::fs::write(&source_path, source).expect("write native Windows fixture source");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc)
        .arg("--edition=2024")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("run rustc for native Windows version fixture");
    assert!(
        output.status.success(),
        "rustc fixture failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    binary_path
}

fn fixture_core() -> Core {
    Core {
        version: "v1.2.3".to_string(),
        repo: "example/tool".to_string(),
        binary_name: "tool".to_string(),
        asset_prefix: "tool-release".to_string(),
        has_cosign: false,
        release_workflow: String::new(),
        sha256: BTreeMap::new(),
        platforms: Vec::new(),
        asset_arch: String::new(),
        optional: false,
    }
}

fn fixture_routes(core: &Core, platform: Platform, archive: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let alternate = core.asset_name_alt(platform.os, platform.arch);
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    let digest = format!("{:x}", Sha256::digest(archive));
    BTreeMap::from([
        (format!("{prefix}{alternate}"), archive.to_vec()),
        (
            format!("{prefix}checksums.txt"),
            format!("{digest}  {alternate}\n").into_bytes(),
        ),
    ])
}

#[test]
fn installer_downloads_fallback_asset_verifies_and_installs() {
    let core = fixture_core();
    let platform = Platform::current().unwrap();
    let binary = fixture_binary(platform, b"#!/bin/sh\necho native\n");
    let archive = archive(platform, &core.binary_name, binary);
    let server = TestServer::start(fixture_routes(&core, platform, &archive));
    let temp = tempfile::tempdir().unwrap();
    let mut warnings = Vec::new();
    let installer = Installer::with_base_url(temp.path(), false, &server.url).unwrap();

    if let Err(error) = installer.install(&core, platform, &mut warnings) {
        panic!(
            "install failed: {error}; requests={:?}",
            server.requests.lock().unwrap()
        );
    }

    assert_eq!(std::fs::read(temp.path().join("tool")).unwrap(), binary);
    assert!(warnings.is_empty());
    let requests = server.requests.lock().unwrap();
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    assert!(requests.contains(&format!("{prefix}checksums.txt")));
    assert!(requests.contains(&format!(
        "{prefix}{}",
        core.asset_name(platform.os, platform.arch)
    )));
    assert!(requests.contains(&format!(
        "{prefix}{}",
        core.asset_name_alt(platform.os, platform.arch)
    )));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(temp.path().join("tool"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}

#[test]
fn installer_prefers_versioned_asset_with_manifest_pin() {
    let mut core = fixture_core();
    let platform = Platform::current().unwrap();
    let binary = fixture_binary(platform, b"versioned");
    let archive = archive(platform, &core.binary_name, binary);
    let primary = core.asset_name(platform.os, platform.arch);
    core.sha256
        .insert(primary.clone(), format!("{:x}", Sha256::digest(&archive)));
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    let server = TestServer::start(BTreeMap::from([(format!("{prefix}{primary}"), archive)]));
    let temp = tempfile::tempdir().unwrap();
    let installer = Installer::with_base_url(temp.path(), false, &server.url).unwrap();

    installer.install(&core, platform, &mut Vec::new()).unwrap();

    assert_eq!(std::fs::read(temp.path().join("tool")).unwrap(), binary);
    assert!(server.requests.lock().unwrap().contains(&format!(
        "/{}/releases/download/{}/{}",
        core.repo,
        core.tag(),
        primary
    )));
}

#[test]
fn pinned_primary_mismatch_does_not_downgrade_to_alternate() {
    let mut core = fixture_core();
    let platform = Platform::current().unwrap();
    let archive = archive(platform, &core.binary_name, b"untrusted replacement");
    let primary = core.asset_name(platform.os, platform.arch);
    let alternate = core.asset_name_alt(platform.os, platform.arch);
    core.sha256.insert(primary.clone(), "0".repeat(64));
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    let digest = format!("{:x}", Sha256::digest(&archive));
    let server = TestServer::start(BTreeMap::from([
        (format!("{prefix}{primary}"), archive.clone()),
        (format!("{prefix}{alternate}"), archive),
        (
            format!("{prefix}checksums.txt"),
            format!("{digest}  {alternate}\n").into_bytes(),
        ),
    ]));
    let temp = tempfile::tempdir().unwrap();
    let installer = Installer::with_base_url(temp.path(), false, &server.url).unwrap();

    let error = match installer.install(&core, platform, &mut Vec::new()) {
        Ok(outcome) => panic!(
            "unexpected {outcome:?}; requests={:?}",
            server.requests.lock().unwrap()
        ),
        Err(error) => error,
    };

    assert!(error.to_string().contains("pinned checksum"));
    assert!(!temp.path().join("tool").exists());
    let requests = server.requests.lock().unwrap();
    assert!(requests.contains(&format!("{prefix}{primary}")));
    assert!(!requests.contains(&format!("{prefix}{alternate}")));
}

#[test]
fn checksum_failure_leaves_existing_binary_untouched() {
    let mut core = fixture_core();
    let platform = Platform::current().unwrap();
    let archive = archive(platform, &core.binary_name, b"replacement");
    let alternate = core.asset_name_alt(platform.os, platform.arch);
    core.sha256.insert(alternate.clone(), "0".repeat(64));
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    let server = TestServer::start(BTreeMap::from([(format!("{prefix}{alternate}"), archive)]));
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("tool"), b"existing").unwrap();
    let installer = Installer::with_base_url(temp.path(), false, &server.url).unwrap();

    assert!(installer.install(&core, platform, &mut Vec::new()).is_err());
    assert_eq!(
        std::fs::read(temp.path().join("tool")).unwrap(),
        b"existing"
    );
}

#[test]
#[cfg(unix)]
fn installed_version_probes_json_and_normalizes_v_prefix() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("tool");
    std::fs::write(&binary, b"#!/bin/sh\nprintf '{\"version\":\"1.2.3\"}\\n'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let version = installed_version(temp.path(), "tool").unwrap();
    assert_eq!(version, "1.2.3");
    assert!(versions_match(&version, "v1.2.3"));
    assert_eq!(
        installed_version(Path::new("/missing"), "tool").unwrap(),
        ""
    );
}

#[test]
#[cfg(windows)]
fn installed_version_probes_native_windows_executable() {
    let temp = tempfile::tempdir().unwrap();
    let binary = compile_version_probe(
        temp.path(),
        "version-fixture",
        r##"fn main() {
            println!(r#"{"version":"1.2.3"}"#);
        }"##,
    );
    assert!(
        std::process::Command::new(&binary)
            .args(["version", "--json"])
            .output()
            .expect("run native fixture")
            .status
            .success()
    );

    let bin_dir = tempfile::tempdir().unwrap();
    let installed = bin_dir.path().join("tool");
    std::fs::copy(binary, installed).expect("install native probe fixture");
    let version = installed_version(bin_dir.path(), "tool").unwrap();
    assert_eq!(version, "1.2.3");
    assert!(versions_match(&version, "v1.2.3"));
    assert_eq!(
        installed_version(&temp.path().join("absent"), "tool").unwrap(),
        ""
    );
}

#[test]
#[cfg(windows)]
fn installed_version_timeout_kills_native_windows_executable() {
    use std::time::Duration;

    let temp = tempfile::tempdir().unwrap();
    let binary = compile_version_probe(
        temp.path(),
        "slow-version-fixture",
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(20)); }",
    );
    let bin_dir = tempfile::tempdir().unwrap();
    let installed = bin_dir.path().join("tool");
    std::fs::copy(binary, installed).expect("install slow native probe fixture");

    let started = std::time::Instant::now();
    let error = installed_version(bin_dir.path(), "tool").unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
#[cfg(unix)]
fn installed_version_timeout_is_bounded_and_reaps_descendants() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let temp = tempfile::tempdir().unwrap();
    let binary = temp.path().join("tool");
    let marker = temp.path().join("child.pid");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nsleep 20 &\necho $! > '{}'\nwait\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let started = Instant::now();
    let error = installed_version(temp.path(), "tool").unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(5));

    let pid = std::fs::read_to_string(marker).unwrap();
    let alive = Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.trim())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(!alive, "version-probe descendant survived timeout");
}
