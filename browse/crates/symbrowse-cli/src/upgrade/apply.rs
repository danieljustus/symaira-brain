//! Apply half of `symbrowse upgrade`: install-method detection, asset
//! selection, checksum verification and the atomic swap with backup/rollback
//! (the behavior of the Go corekit `updateapply` + `installmethod` packages
//! as wired by `browse/cmd/symbrowse/upgrade.go`).

use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};

use super::Release;

const MAX_ASSET_BODY: usize = 1 << 30;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InstallMethod {
    DirectDownload,
    Homebrew,
    GoInstall,
    PackageManager,
    BuildFromSource,
    Unknown,
}

impl InstallMethod {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::DirectDownload => "direct-download",
            Self::Homebrew => "homebrew",
            Self::GoInstall => "go-install",
            Self::PackageManager => "package-manager",
            Self::BuildFromSource => "build-from-source",
            Self::Unknown => "unknown",
        }
    }

    pub(super) fn guidance(self, tool: &str) -> String {
        match self {
            Self::DirectDownload => {
                "Re-run the quick install script or download the latest release from GitHub".into()
            }
            Self::Homebrew => format!("Update via Homebrew: brew update && brew upgrade {tool}"),
            Self::GoInstall => {
                format!("Update via Go: go install github.com/danieljustus/{tool}@latest")
            }
            Self::PackageManager => "Update via your system package manager (e.g., apt upgrade, yum update, pacman -Syu)".into(),
            Self::BuildFromSource => "Rebuild from source: git pull && go build".into(),
            Self::Unknown => {
                "Unable to determine installation method. Reinstall from the GitHub releases page"
                    .into()
            }
        }
    }
}

fn resolved(path: &str) -> String {
    fs::canonicalize(path).map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into_owned())
}

pub(super) fn detect(binary: &Path) -> Result<InstallMethod, String> {
    if binary.as_os_str().is_empty() {
        return Err("binary path must not be empty".to_owned());
    }
    let real = fs::canonicalize(binary).unwrap_or_else(|_| binary.to_path_buf());
    let abs = if real.is_absolute() {
        real
    } else {
        env::current_dir().map_or(real.clone(), |cwd| cwd.join(&real))
    };
    let abs = abs.to_string_lossy().into_owned();
    let home = env::var("HOME").ok().filter(|h| !h.is_empty());
    let env_dir = |name: &str| env::var(name).ok().filter(|v| !v.is_empty());

    if let Some(prefix) = env_dir("HOMEBREW_PREFIX")
        && abs.starts_with(&resolved(&prefix))
    {
        return Ok(InstallMethod::Homebrew);
    }
    if let Some(gopath) = env_dir("GOPATH")
        && abs.starts_with(
            &Path::new(&resolved(&gopath))
                .join("bin")
                .to_string_lossy()
                .into_owned(),
        )
    {
        return Ok(InstallMethod::GoInstall);
    }
    if let Some(cache) = env_dir("GOMODCACHE")
        && abs.starts_with(&resolved(&cache))
    {
        return Ok(InstallMethod::GoInstall);
    }

    if abs.contains("/opt/homebrew/")
        || abs.contains("/usr/local/Cellar/")
        || abs.contains("/.linuxbrew/")
    {
        return Ok(InstallMethod::Homebrew);
    }
    if abs.starts_with("/usr/bin/") {
        return Ok(InstallMethod::PackageManager);
    }
    if abs.starts_with("/usr/local/bin/") {
        return Ok(InstallMethod::DirectDownload);
    }
    if let Some(gopath) = env_dir("GOPATH") {
        let bin = format!("{}/", Path::new(&resolved(&gopath)).join("bin").display());
        if abs.starts_with(&bin) {
            return Ok(InstallMethod::GoInstall);
        }
    }
    if let Some(home) = &home {
        if abs.starts_with(&format!("{home}/go/bin/")) {
            return Ok(InstallMethod::GoInstall);
        }
        for dir in ["bin", ".local/bin", ".cargo/bin"] {
            if abs.starts_with(&format!("{home}/{dir}/")) {
                return Ok(InstallMethod::DirectDownload);
            }
        }
    }

    if abs.contains("/Cellar/") {
        return Ok(InstallMethod::Homebrew);
    }
    let base = Path::new(&abs)
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    if Path::new(&format!("/var/lib/dpkg/info/{base}.list")).exists() {
        return Ok(InstallMethod::PackageManager);
    }
    if abs.contains('@') || abs.contains("/pkg/mod/") {
        return Ok(InstallMethod::GoInstall);
    }

    if cfg!(windows) {
        return Ok(InstallMethod::BuildFromSource);
    }
    let Some(dir) = Path::new(&abs).parent() else {
        return Ok(InstallMethod::Unknown);
    };
    let Ok(meta) = fs::metadata(dir) else {
        return Ok(InstallMethod::Unknown);
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode()
    };
    #[cfg(not(unix))]
    let mode = if meta.permissions().readonly() {
        0o444
    } else {
        0o644
    };
    if mode & 0o200 != 0 || mode & 0o022 != 0 {
        Ok(InstallMethod::DirectDownload)
    } else {
        Ok(InstallMethod::BuildFromSource)
    }
}

pub(super) fn go_os() -> &'static str {
    match env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

pub(super) fn go_arch() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        other => other,
    }
}

fn select_asset<'a>(
    release: &'a Release,
    goos: &str,
    goarch: &str,
) -> Result<&'a super::Asset, String> {
    release
        .assets
        .iter()
        .find(|asset| {
            let name = asset.name.to_lowercase();
            !name.contains("checksums") && name.contains(goos) && name.contains(goarch)
        })
        .ok_or_else(|| format!("updateapply: no release asset matches {goos}/{goarch}"))
}

async fn download(
    client: &reqwest::Client,
    asset: &super::Asset,
) -> Result<reqwest::Response, String> {
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|error| format!("request asset: {error}"))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!(
            "download {:?}: HTTP {}",
            asset.name,
            response.status().as_u16()
        ));
    }
    Ok(response)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if super::is_github_host(attempt.url().host_str().unwrap_or_default())
                && attempt.previous().len() < 10
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .build()
        .map_err(|error| format!("create HTTP client: {error}"))
}

fn parse_checksums(data: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let sums: std::collections::HashMap<String, String> = data
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 2).then(|| (fields[1].to_owned(), fields[0].to_owned()))
        })
        .collect();
    if sums.is_empty() {
        return Err("checksums.txt contained no parseable entries".to_owned());
    }
    Ok(sums)
}

fn check_writable(target: &Path) -> Result<(), String> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let probe = dir.join(".updateapply-writecheck");
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&probe)
        .map_err(|error| {
            format!(
                "install location {:?} is not writable: {error}",
                dir.display()
            )
        })?;
    let _ = fs::remove_file(&probe);
    #[cfg(unix)]
    if let Ok(meta) = fs::metadata(target) {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o200 == 0 {
            return Err(format!(
                "target binary {:?} is not writable",
                target.display()
            ));
        }
    }
    Ok(())
}

fn atomic_swap(new_path: &Path, target: &Path) -> Result<(), String> {
    let backup = PathBuf::from(format!("{}.bak", target.display()));
    let had_existing = fs::metadata(target).is_ok();
    if had_existing {
        let _ = fs::remove_file(&backup);
        fs::rename(target, &backup).map_err(|error| format!("backup current binary: {error}"))?;
    }
    if let Err(error) = fs::rename(new_path, target) {
        if had_existing && let Err(rollback) = fs::rename(&backup, target) {
            return Err(format!(
                "install new binary failed ({error}) and rollback failed ({rollback})"
            ));
        }
        return Err(format!("install new binary: {error}"));
    }
    if had_existing {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

/// `updateapply.Applier.Apply` with `CheckInstallMethod` and no cosign or
/// archive extraction, exactly like the Go CLI configures it.
pub(super) fn apply(release: &Release, target: &Path) -> Result<(), String> {
    let method =
        detect(target).map_err(|error| format!("updateapply: detect install method: {error}"))?;
    if method != InstallMethod::DirectDownload {
        return Err(format!(
            "updateapply: self-update is not supported for {} installation — {}",
            method.as_str(),
            method.guidance(
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .as_str()
            )
        ));
    }
    let asset = select_asset(release, go_os(), go_arch())?;
    let checksum_asset = release
        .assets
        .iter()
        .find(|asset| asset.name.to_lowercase().contains("checksums"))
        .ok_or_else(|| {
            "updateapply: fetch checksums: release has no checksums.txt asset".to_owned()
        })?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("updateapply: create runtime: {error}"))?;
    let client = client().map_err(|error| format!("updateapply: {error}"))?;

    let sums = runtime
        .block_on(async {
            let mut response = download(&client, checksum_asset).await?;
            let mut data = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| format!("read checksums.txt: {error}"))?
            {
                if data.len() + chunk.len() > MAX_ASSET_BODY {
                    break;
                }
                data.extend_from_slice(&chunk);
            }
            parse_checksums(&String::from_utf8_lossy(&data))
        })
        .map_err(|error| format!("updateapply: fetch checksums: {error}"))?;
    let want = sums
        .get(&asset.name)
        .ok_or_else(|| format!("updateapply: no checksum entry for asset {:?}", asset.name))?;

    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|e| format!("updateapply: resolve target path: {e}"))?
            .join(target)
    };
    check_writable(&abs).map_err(|error| format!("updateapply: {error}"))?;
    let staging = abs.parent().unwrap_or(Path::new(".")).to_path_buf();
    let staged = staging.join(format!(".updateapply-{}.tmp", std::process::id()));
    let mut tmp = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|error| format!("updateapply: download asset: create temp file: {error}"))?;
    let _cleanup = RemoveOnDrop(staged.clone());
    let got = runtime
        .block_on(async {
            let mut response = download(&client, asset).await?;
            let total = response.content_length().unwrap_or(0);
            let mut hasher = Sha256::new();
            let mut written: u64 = 0;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| format!("read asset body: {error}"))?
            {
                tmp.write_all(&chunk)
                    .map_err(|error| format!("write temp file: {error}"))?;
                hasher.update(&chunk);
                written += chunk.len() as u64;
                if written as usize > MAX_ASSET_BODY {
                    break;
                }
            }
            tmp.flush()
                .map_err(|error| format!("write temp file: {error}"))?;
            if total > 0 && written != total {
                return Err(format!(
                    "incomplete download: got {written} bytes, want {total}"
                ));
            }
            Ok::<_, String>(
                hasher
                    .finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            )
        })
        .map_err(|error| format!("updateapply: download asset: {error}"))?;
    if !got.eq_ignore_ascii_case(want) {
        return Err(format!(
            "updateapply: checksum mismatch for {:?}: got {got}, want {want}",
            asset.name
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("updateapply: make downloaded asset executable: {error}"))?;
    }
    drop(tmp);
    atomic_swap(&staged, &abs)
}

/// Removes the staged download on every exit path (after a successful swap
/// the path no longer exists and the removal is a no-op).
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
