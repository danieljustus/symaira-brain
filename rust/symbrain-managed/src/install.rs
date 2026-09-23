//! Download, publisher verification, installation, and installed-version probes.

use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::Agent;

use crate::{
    Core, ManagedError, Platform, atomic_install, download_url, extract_binary, find_checksum,
    normalize_version, verify_checksum,
};

const DEFAULT_BASE_URL: &str = "https://github.com";
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    SkippedPlatform,
}

#[derive(Debug)]
pub struct Installer {
    bin_dir: PathBuf,
    temp_dir: Option<PathBuf>,
    base_url: String,
    allow_unsigned: bool,
    client: Agent,
}

impl Installer {
    /// Creates an installer using the production release host.
    ///
    /// `SYMBRAIN_RELEASE_BASE_URL` is an explicit fixture seam used by the
    /// cross-runtime differential harness; checksum and publisher verification
    /// remain active when it is set.
    ///
    /// # Errors
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(bin_dir: impl Into<PathBuf>, allow_unsigned: bool) -> Result<Self, ManagedError> {
        let base_url =
            env::var("SYMBRAIN_RELEASE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(bin_dir, allow_unsigned, base_url)
    }

    /// Creates an installer against an explicit release base URL.
    ///
    /// # Errors
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn with_base_url(
        bin_dir: impl Into<PathBuf>,
        allow_unsigned: bool,
        base_url: impl Into<String>,
    ) -> Result<Self, ManagedError> {
        Ok(Self {
            bin_dir: bin_dir.into(),
            temp_dir: None,
            base_url: base_url.into(),
            allow_unsigned,
            client: ureq::agent(),
        })
    }

    #[must_use]
    pub fn with_temp_dir(mut self, temp_dir: impl Into<PathBuf>) -> Self {
        self.temp_dir = Some(temp_dir.into());
        self
    }

    /// Downloads, verifies, extracts, and atomically installs one core.
    ///
    /// # Errors
    /// Returns an error without changing the destination binary if any network,
    /// integrity, publisher, archive, or filesystem step fails.
    pub fn install(
        &self,
        core: &Core,
        platform: Platform,
        warn: &mut dyn Write,
    ) -> Result<InstallOutcome, ManagedError> {
        if !core.supports_platform(platform.os) {
            return Ok(InstallOutcome::SkippedPlatform);
        }

        let download_dir = match &self.temp_dir {
            Some(root) => tempfile::Builder::new()
                .prefix("symbrain-managed-")
                .tempdir_in(root),
            None => tempfile::Builder::new()
                .prefix("symbrain-managed-")
                .tempdir(),
        }
        .map_err(|error| ManagedError::IoContext("create temp dir".to_string(), error))?;

        let checksums_path = download_dir.path().join("checksums.txt");
        let checksums_url = download_url(
            &self.base_url,
            &core.repo,
            &core.tag(),
            &core.checksum_asset_name(),
        );
        if self.download_file(&checksums_url, &checksums_path).is_err() {
            let alternate = download_url(
                &self.base_url,
                &core.repo,
                &core.tag(),
                core.checksum_asset_name_alt(),
            );
            let _ = self.download_file(&alternate, &checksums_path);
        }

        let primary = core.asset_name(platform.os, platform.arch);
        let alternate = core.asset_name_alt(platform.os, platform.arch);
        let archive_result = match self.download_and_verify(
            core,
            &primary,
            &checksums_path,
            download_dir.path(),
            warn,
        ) {
            Ok(path) => Ok(path),
            Err(error) if primary != alternate && error.is_download_not_found() => self
                .download_and_verify(core, &alternate, &checksums_path, download_dir.path(), warn),
            Err(error) => Err(error),
        };
        let archive = archive_result.map_err(|error| {
            ManagedError::Context(format!("managed: download {}: {error}", core.binary_name))
        })?;

        let binary =
            extract_binary(&archive, core, platform.os, platform.arch).map_err(|error| {
                ManagedError::Context(format!("managed: extract {}: {error}", core.binary_name))
            })?;
        fs::create_dir_all(&self.bin_dir).map_err(|error| {
            ManagedError::IoContext(format!("mkdir {}", self.bin_dir.display()), error)
        })?;
        atomic_install(&self.bin_dir, &core.binary_name, &binary)?;
        record_release_provenance(&self.bin_dir, core, &binary).map_err(|error| {
            ManagedError::Context(format!(
                "managed: record provenance for {}: {error}",
                core.binary_name
            ))
        })?;
        Ok(InstallOutcome::Installed)
    }

    fn download_and_verify(
        &self,
        core: &Core,
        asset: &str,
        checksums_path: &Path,
        download_dir: &Path,
        warn: &mut dyn Write,
    ) -> Result<PathBuf, ManagedError> {
        let url = download_url(&self.base_url, &core.repo, &core.tag(), asset);
        let archive = download_dir.join(asset);
        self.download_file(&url, &archive)?;

        if let Some(expected) = core.pinned_checksum(asset) {
            verify_checksum(&archive, expected)
                .map_err(|error| ManagedError::Checksum(format!("pinned checksum: {error}")))?;
        } else {
            let text = fs::read_to_string(checksums_path).map_err(|error| {
                ManagedError::Checksum(format!("checksum lookup: checksums: open: {error}"))
            })?;
            let expected = find_checksum(&text, asset)
                .map_err(|error| ManagedError::Checksum(format!("checksum lookup: {error}")))?;
            verify_checksum(&archive, &expected)?;
        }

        if core.has_cosign {
            let signature = PathBuf::from(format!("{}.sig", archive.display()));
            let certificate = PathBuf::from(format!("{}.pem", archive.display()));
            let signature_url = download_url(
                &self.base_url,
                &core.repo,
                &core.tag(),
                &format!("{asset}.sig"),
            );
            let certificate_url = download_url(
                &self.base_url,
                &core.repo,
                &core.tag(),
                &format!("{asset}.pem"),
            );
            let _ = self.download_file(&signature_url, &signature);
            let _ = self.download_file(&certificate_url, &certificate);
            verify_cosign(
                &archive,
                &signature,
                &certificate,
                core,
                self.allow_unsigned,
                warn,
            )?;
        }

        Ok(archive)
    }

    fn download_file(&self, url: &str, destination: &Path) -> Result<u64, ManagedError> {
        let mut response = self
            .client
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|error| match error {
                ureq::Error::StatusCode(404) => {
                    ManagedError::DownloadNotFound(format!("{url}: HTTP 404"))
                }
                ureq::Error::StatusCode(code) => {
                    ManagedError::Download(format!("{url}: HTTP {code}"))
                }
                other => ManagedError::Download(format!("{url}: {other}")),
            })?;

        let mut file = File::create(destination).map_err(|error| {
            ManagedError::Download(format!("create {}: {error}", destination.display()))
        })?;
        let copied =
            std::io::copy(&mut response.body_mut().as_reader(), &mut file).map_err(|error| {
                let _ = fs::remove_file(destination);
                ManagedError::Download(format!("write {}: {error}", destination.display()))
            })?;
        Ok(copied)
    }
}

#[derive(Serialize)]
struct ReleaseProvenance<'a> {
    binary: &'a str,
    source: &'static str,
    version: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    repo: &'a str,
    built_at: String,
    binary_sha256: String,
}

fn record_release_provenance(
    bin_dir: &Path,
    core: &Core,
    binary: &[u8],
) -> Result<(), ManagedError> {
    let provenance = ReleaseProvenance {
        binary: &core.binary_name,
        source: "release",
        version: &core.version,
        repo: &core.repo,
        built_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        binary_sha256: format!("{:x}", Sha256::digest(binary)),
    };
    let mut data = serde_json::to_vec_pretty(&provenance)
        .map_err(|error| ManagedError::Context(format!("managed: marshal provenance: {error}")))?;
    data.push(b'\n');

    let mut temporary = tempfile::NamedTempFile::new_in(bin_dir).map_err(|error| {
        ManagedError::Context(format!("managed: create provenance temp: {error}"))
    })?;
    temporary
        .write_all(&data)
        .map_err(|error| ManagedError::Context(format!("managed: write provenance: {error}")))?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| ManagedError::Context(format!("managed: sync provenance: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|error| {
                ManagedError::Context(format!("managed: chmod provenance: {error}"))
            })?;
    }
    let target = bin_dir.join(format!("{}.provenance.json", core.binary_name));
    temporary.persist(target).map_err(|error| {
        ManagedError::Context(format!("managed: rename provenance: {}", error.error))
    })?;
    Ok(())
}

#[cfg(test)]
mod provenance_tests {
    use std::fs;

    use serde_json::Value;

    use super::record_release_provenance;
    use crate::Core;

    #[test]
    fn release_provenance_matches_go_sidecar_contract() {
        let bin_dir = tempfile::tempdir().expect("temporary bin directory");
        let core = Core {
            version: "v1.2.3".to_string(),
            repo: "owner/tool".to_string(),
            binary_name: "symtool".to_string(),
            ..Core::default()
        };
        let binary = b"installed binary payload";

        record_release_provenance(bin_dir.path(), &core, binary).expect("write provenance");

        let path = bin_dir.path().join("symtool.provenance.json");
        let bytes = fs::read(&path).expect("read sidecar");
        assert!(bytes.ends_with(b"\n"), "Go sidecars end with a newline");
        let text = std::str::from_utf8(&bytes).expect("sidecar UTF-8");
        let mut remaining = text;
        for key in [
            "\"binary\"",
            "\"source\"",
            "\"version\"",
            "\"repo\"",
            "\"built_at\"",
            "\"binary_sha256\"",
        ] {
            let index = remaining.find(key).expect("Go-compatible field order");
            remaining = &remaining[index + key.len()..];
        }
        let value: Value = serde_json::from_slice(&bytes).expect("parse sidecar");
        assert_eq!(value.as_object().expect("sidecar object").len(), 6);
        assert_eq!(value["binary"], "symtool");
        assert_eq!(value["source"], "release");
        assert_eq!(value["version"], "v1.2.3");
        assert_eq!(value["repo"], "owner/tool");
        assert!(
            value["built_at"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
        );
        assert_eq!(
            value["binary_sha256"],
            "e145acca56d558b65847ef309f65716afbc714552e7eac95ed0c7ac6371c16c8"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o644
            );
        }
    }
}

fn verify_cosign(
    artifact: &Path,
    signature: &Path,
    certificate: &Path,
    core: &Core,
    allow_unsigned: bool,
    warn: &mut dyn Write,
) -> Result<(), ManagedError> {
    let cosign = find_on_path("cosign");
    let reason = if cosign.is_none() {
        #[cfg(windows)]
        let lookup_error = "executable file not found in %PATH%";
        #[cfg(not(windows))]
        let lookup_error = "executable file not found in $PATH";
        Some(format!(
            "cosign is not installed on PATH (exec: \"cosign\": {lookup_error})"
        ))
    } else if !signature.is_file() {
        Some(format!("signature file missing: {}", signature.display()))
    } else if !certificate.is_file() {
        Some(format!(
            "certificate file missing: {}",
            certificate.display()
        ))
    } else {
        None
    };

    if let Some(reason) = reason {
        if allow_unsigned {
            writeln!(
                warn,
                "WARNING: skipping cosign verification for {} (--allow-unsigned): {reason} — the publisher was NOT authenticated",
                artifact.file_name().unwrap_or_default().to_string_lossy()
            )
            .map_err(ManagedError::Io)?;
            return Ok(());
        }
        return Err(ManagedError::Cosign(format!(
            "cannot verify publisher signature for {}: {reason} (pass --allow-unsigned to install anyway at your own risk)",
            artifact.file_name().unwrap_or_default().to_string_lossy()
        )));
    }

    let combined = tempfile::NamedTempFile::new()
        .map_err(|error| ManagedError::Cosign(format!("create output capture: {error}")))?;
    let stdout = combined
        .as_file()
        .try_clone()
        .map_err(|error| ManagedError::Cosign(format!("clone output capture: {error}")))?;
    let stderr = combined
        .as_file()
        .try_clone()
        .map_err(|error| ManagedError::Cosign(format!("clone output capture: {error}")))?;
    let status = Command::new(cosign.expect("checked above"))
        .arg("verify-blob")
        .arg(artifact)
        .arg("--signature")
        .arg(signature)
        .arg("--certificate")
        .arg(certificate)
        .arg("--certificate-identity")
        .arg(core.certificate_identity())
        .arg("--certificate-oidc-issuer")
        .arg(core.certificate_oidc_issuer())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .map_err(|error| ManagedError::Cosign(format!("start cosign: {error}")))?;
    if !status.success() {
        let output = fs::read(combined.path())
            .map_err(|error| ManagedError::Cosign(format!("read cosign output: {error}")))?;
        return Err(ManagedError::Cosign(format!(
            "cosign verify: {}: {} (output: {})",
            artifact.display(),
            format_exit_status(status),
            String::from_utf8_lossy(&output)
        )));
    }
    Ok(())
}

fn format_exit_status(status: std::process::ExitStatus) -> String {
    status.code().map_or_else(
        || "signal: killed".to_string(),
        |code| format!("exit status {code}"),
    )
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let names = executable_names(binary);
    env::split_paths(&path)
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

fn executable_names(binary: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let extensions = env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        let mut names = vec![OsString::from(binary)];
        names.extend(
            extensions
                .to_string_lossy()
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| OsString::from(format!("{binary}{extension}"))),
        );
        names
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(binary)]
    }
}

#[derive(Deserialize)]
struct VersionPayload {
    version: String,
}

/// Probes `<binary> version --json`, returning an empty string when absent.
///
/// # Errors
/// Returns an error when the process cannot start, times out, exits non-zero,
/// or emits malformed JSON.
pub fn installed_version(bin_dir: &Path, binary_name: &str) -> Result<String, ManagedError> {
    let path = bin_dir.join(binary_name);
    if !path.exists() {
        return Ok(String::new());
    }

    let output = tempfile::NamedTempFile::new()
        .map_err(|error| ManagedError::IoContext("create version output".to_string(), error))?;
    let output_writer = output
        .reopen()
        .map_err(|error| ManagedError::IoContext("open version output".to_string(), error))?;
    let mut command = Command::new(&path);
    command
        .arg("version")
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_writer))
        .stderr(Stdio::null());
    configure_probe_process(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ManagedError::Process(format!("probe {binary_name}: {error}")))?;
    let process_group = child.id();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate_probe_descendants(process_group);
                let bytes = fs::read(output.path()).map_err(ManagedError::Io)?;
                if !status.success() {
                    return Err(ManagedError::Process(format!(
                        "probe {binary_name}: process exited with {status}"
                    )));
                }
                let payload: VersionPayload = serde_json::from_slice(&bytes).map_err(|error| {
                    ManagedError::Process(format!("parse {binary_name} version: {error}"))
                })?;
                return Ok(payload.version);
            }
            Ok(None) if started.elapsed() < VERSION_PROBE_TIMEOUT => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                terminate_probe(&mut child, process_group);
                return Err(ManagedError::Process(format!(
                    "probe {binary_name}: timed out"
                )));
            }
            Err(error) => {
                terminate_probe(&mut child, process_group);
                return Err(ManagedError::Process(format!(
                    "probe {binary_name}: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn configure_probe_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_probe_process(_command: &mut Command) {}

fn terminate_probe(child: &mut std::process::Child, process_group: u32) {
    #[cfg(unix)]
    {
        signal_probe_group(process_group, "-TERM");
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                terminate_probe_descendants(process_group);
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        terminate_probe_descendants(process_group);
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        let _ = process_group;
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(unix)]
fn terminate_probe_descendants(process_group: u32) {
    signal_probe_group(process_group, "-KILL");
}

#[cfg(not(unix))]
fn terminate_probe_descendants(_process_group: u32) {}

#[cfg(unix)]
fn signal_probe_group(process_group: u32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{process_group}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[must_use]
pub fn versions_match(installed: &str, wanted: &str) -> bool {
    normalize_version(installed) == normalize_version(wanted)
}
