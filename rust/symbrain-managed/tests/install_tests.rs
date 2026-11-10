use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use symbrain_managed::{Core, Installer, Platform, installed_version, versions_match};

#[path = "support/server.rs"]
mod server;
use server::TestServer;

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

/// Serializes every test that writes an executable fixture and then execs it
/// (#927). `fork` copies the whole fd table, so while another test thread is
/// between `fork` and `exec` it holds a duplicate of any fixture's write
/// descriptor that is still open on a sibling thread; on Linux the sibling's
/// `exec` of that fixture then fails with ETXTBSY ("text file busy") before the
/// intended probe ever starts. Holding this lock across write + probe means no
/// other probe can fork inside that window. Nothing is retried and no assertion
/// is relaxed: a genuine ETXTBSY would still fail the test.
fn exec_fixture_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    let digest = symbrain_core::hex::lower(Sha256::digest(archive));
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
fn installer_retries_alternate_after_http10_closes_missing_asset_response() {
    let core = fixture_core();
    let platform = Platform::current().unwrap();
    let binary = fixture_binary(platform, b"native HTTP/1.0 fallback");
    let archive = archive(platform, &core.binary_name, binary);
    let server = TestServer::start_http10(fixture_routes(&core, platform, &archive));
    let temp = tempfile::tempdir().unwrap();
    let installer = Installer::with_base_url(temp.path(), false, &server.url).unwrap();
    for _ in 0..3 {
        let mut warnings = Vec::new();
        installer.install(&core, platform, &mut warnings).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(std::fs::read(temp.path().join("tool")).unwrap(), binary);
    }
    let prefix = format!("/{}/releases/download/{}/", core.repo, core.tag());
    let primary = format!("{prefix}{}", core.asset_name(platform.os, platform.arch));
    let alternate = format!(
        "{prefix}{}",
        core.asset_name_alt(platform.os, platform.arch)
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|path| **path == primary).count(), 3);
    assert_eq!(
        requests.iter().filter(|path| **path == alternate).count(),
        3
    );
}

#[test]
fn installer_prefers_versioned_asset_with_manifest_pin() {
    let mut core = fixture_core();
    let platform = Platform::current().unwrap();
    let binary = fixture_binary(platform, b"versioned");
    let archive = archive(platform, &core.binary_name, binary);
    let primary = core.asset_name(platform.os, platform.arch);
    core.sha256.insert(
        primary.clone(),
        symbrain_core::hex::lower(Sha256::digest(&archive)),
    );
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
    let digest = symbrain_core::hex::lower(Sha256::digest(&archive));
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
fn installed_version_probes_json_and_normalizes_v_prefix() {
    use std::os::unix::fs::PermissionsExt;

    let _exec_fixture = exec_fixture_lock();
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
fn installed_version_timeout_is_bounded_and_reaps_descendants() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let _exec_fixture = exec_fixture_lock();
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
    assert_eq!(
        error.to_string(),
        "probe tool: signal: killed",
        "unexpected probe error: {error}; debug={error:?}; elapsed={:?}; fixture={binary:?}; metadata={:?}; child_marker={:?}",
        started.elapsed(),
        std::fs::metadata(&binary).map(|metadata| (metadata.len(), metadata.permissions().mode())),
        std::fs::read_to_string(&marker),
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "probe exceeded bound: {error}; elapsed={:?}; child_marker={:?}",
        started.elapsed(),
        std::fs::read_to_string(&marker),
    );

    let pid = std::fs::read_to_string(&marker).unwrap_or_else(|marker_error| {
        panic!("missing descendant marker: {marker_error}; probe={error}; fixture={binary:?}")
    });
    let alive = Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.trim())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(
        !alive,
        "version-probe descendant survived timeout: pid={pid:?}; probe={error}; elapsed={:?}; fixture={binary:?}",
        started.elapsed(),
    );
}

/// Root cause of #927, reproduced deterministically: Linux refuses to exec a
/// file while any process still holds it open for writing, which is what a
/// concurrently forking sibling thread's inherited descriptor copy amounts to.
/// A holder process keeps such a write descriptor open on purpose.
#[test]
#[cfg(target_os = "linux")]
fn exec_of_fixture_fails_with_etxtbsy_while_a_writer_descriptor_is_inherited() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    const ETXTBSY: i32 = 26;
    let _exec_fixture = exec_fixture_lock();
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("tool");
    std::fs::write(&script, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut holder = Command::new("/bin/sh")
        .args([
            "-c",
            "exec 9>>\"$0\"; echo ready; read _ || true",
            script.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(holder.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "ready");

    let busy = Command::new(&script).spawn().unwrap_err();
    assert_eq!(busy.raw_os_error(), Some(ETXTBSY), "{busy}");

    drop(holder.stdin.take());
    assert!(holder.wait().unwrap().success());
    assert!(Command::new(&script).status().unwrap().success());
}
