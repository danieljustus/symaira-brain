//! Read-only release checks compatible with the Go updatecheck cache contract.

use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration as StdDuration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use symbrowse_core::{
    error::ErrorCode,
    output::{Envelope, Format},
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

const OWNER: &str = "danieljustus";
const REPOSITORY: &str = "symaira-browse";
const CACHE_TTL: Duration = Duration::hours(24);
const REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(3);
const MAX_RESPONSE_BYTES: usize = 1 << 20;
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/danieljustus/symaira-browse/releases/latest";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Asset {
    #[serde(rename = "Name", alias = "name")]
    name: String,
    #[serde(rename = "BrowserDownloadURL", alias = "browser_download_url")]
    browser_download_url: String,
    #[serde(rename = "Size", alias = "size")]
    size: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Release {
    #[serde(rename = "TagName", alias = "tag_name")]
    tag_name: String,
    #[serde(rename = "Body", alias = "body")]
    body: String,
    #[serde(rename = "HTMLURL", alias = "html_url")]
    html_url: String,
    #[serde(rename = "Assets", alias = "assets")]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: i64,
}

#[derive(Deserialize, Serialize)]
struct CacheEntry {
    timestamp: String,
    release: Option<Release>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StableVersion(u64, u64, u64);

pub fn run(format: Format, binary_version: &str) -> ExitCode {
    let current = version_string(binary_version);
    let cache_path = default_cache_path(OWNER, REPOSITORY);
    let checked_at = OffsetDateTime::now_utc();
    let result = check_release(&current, LATEST_RELEASE_URL, &cache_path);
    match result {
        Ok(Some(release)) => render_success(
            format,
            json!({
                "up_to_date": false,
                "current": current,
                "latest": release.tag_name,
                "url": release.html_url,
                "hint": upgrade_hint(&current, &release.tag_name),
            }),
        ),
        Ok(None) => render_success(
            format,
            json!({
                "up_to_date": true,
                "current": current,
                "checked_at": go_timestamp(checked_at),
            }),
        ),
        Err(error) => render_failure(format, error),
    }
}

fn render_success(format: Format, data: Value) -> ExitCode {
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(output) => super::write_stdout(&output),
        Err(_) => ExitCode::from(1),
    }
}

fn render_failure(format: Format, message: String) -> ExitCode {
    let envelope = Envelope::failure(
        ErrorCode::OperationFailed,
        format!("update check failed: {message}"),
    );
    match envelope.render(format) {
        Ok(output) => {
            if format == Format::Text {
                let _ = std::io::stderr().write_all(output.as_bytes());
                ExitCode::from(1)
            } else {
                super::write_stdout(&output)
            }
        }
        Err(_) => ExitCode::from(1),
    }
}

fn version_string(binary_version: &str) -> String {
    if binary_version.is_empty() || binary_version == "dev" {
        return "0.0.0".to_owned();
    }
    if binary_version.starts_with('v') {
        binary_version.to_owned()
    } else {
        format!("v{binary_version}")
    }
}

fn upgrade_hint(current: &str, latest: &str) -> String {
    format!(
        "symbrowse {latest} is available (current: {current}); this build can check for updates but cannot apply them"
    )
}

fn default_cache_path(owner: &str, repository: &str) -> PathBuf {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| user_home().map(|home| home.join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    let mut hash = Sha256::new();
    hash.update(owner.as_bytes());
    hash.update([0]);
    hash.update(repository.as_bytes());
    let digest = hash.finalize();
    let filename = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    base.join("symaira")
        .join("updatecheck")
        .join(format!("{filename}.json"))
}

fn user_home() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn check_release(
    current: &str,
    latest_url: &str,
    cache_path: &Path,
) -> Result<Option<Release>, String> {
    let Some(current_version) = parse_stable_version(current) else {
        return Ok(None);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("create update-check runtime: {error}"))?;
    runtime.block_on(check_release_async(
        current,
        current_version,
        latest_url,
        cache_path,
    ))
}

async fn check_release_async(
    current: &str,
    current_version: StableVersion,
    latest_url: &str,
    cache_path: &Path,
) -> Result<Option<Release>, String> {
    if let Some(entry) = read_cache(cache_path) {
        if cache_is_fresh(&entry.timestamp) {
            if let Some(release) = entry.release {
                if release_is_newer(&release, current_version) {
                    return Ok(Some(release));
                }
                if parse_stable_version(&release.tag_name).is_some() {
                    return Ok(None);
                }
            }
        }
    }

    let release = fetch_latest_release(current, latest_url).await?;
    let entry = CacheEntry {
        timestamp: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| format!("format cache timestamp: {error}"))?,
        release: Some(release.clone()),
    };
    write_cache(cache_path, &entry);
    if release_is_newer(&release, current_version) {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn read_cache(path: &Path) -> Option<CacheEntry> {
    let raw = fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn cache_is_fresh(timestamp: &str) -> bool {
    let Ok(timestamp) = OffsetDateTime::parse(timestamp, &Rfc3339) else {
        return false;
    };
    OffsetDateTime::now_utc() - timestamp < CACHE_TTL
}

fn write_cache(path: &Path, entry: &CacheEntry) {
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(entry) else {
        return;
    };
    let counter = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".updatecheck-{}-{counter}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(mut file) = options.open(&temporary) else {
        return;
    };
    if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
        return;
    }
    let _ = fs::rename(temporary, path);
}

async fn fetch_latest_release(current: &str, latest_url: &str) -> Result<Release, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if is_github_host(attempt.url().host_str().unwrap_or_default())
                && attempt.previous().len() < 10
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .build()
        .map_err(|error| format!("create HTTP client: {error}"))?;
    let response = client
        .get(latest_url.trim())
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", format!("symbrowse-updatecheck/{current}"))
        .send()
        .await
        .map_err(|error| format!("request latest release: {error}"))?;
    if response.status() != reqwest::StatusCode::OK {
        if response.status() == reqwest::StatusCode::FORBIDDEN
            && response
                .headers()
                .get("x-ratelimit-remaining")
                .is_some_and(|value| value.to_str().ok() == Some("0"))
        {
            return Err("GitHub API rate limit exceeded".to_owned());
        }
        return Err(format!(
            "GitHub API returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("read latest release response: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("latest release response exceeds 1 MiB".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let api: ApiRelease = serde_json::from_slice(&body)
        .map_err(|error| format!("decode latest release response: {error}"))?;
    if api.draft {
        return Err("latest release response returned a draft release".to_owned());
    }
    if api.prerelease {
        return Err("latest release response returned a prerelease".to_owned());
    }
    let tag_name = api.tag_name.trim();
    if tag_name.is_empty() {
        return Err("latest release response did not include a tag name".to_owned());
    }
    if parse_stable_version(tag_name).is_none() {
        return Err(format!(
            "latest release tag {tag_name:?} is not a stable semantic version"
        ));
    }
    Ok(Release {
        tag_name: tag_name.to_owned(),
        body: api.body,
        html_url: api.html_url.trim().to_owned(),
        assets: api
            .assets
            .into_iter()
            .map(|asset| Asset {
                name: asset.name,
                browser_download_url: asset.browser_download_url,
                size: asset.size,
            })
            .collect(),
    })
}

fn is_github_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "github.com"
        || host == "api.github.com"
        || host.ends_with(".github.com")
        || host.ends_with(".githubusercontent.com")
}

fn parse_stable_version(raw: &str) -> Option<StableVersion> {
    let raw = raw.trim().strip_prefix('v').unwrap_or(raw.trim());
    if raw.contains('-') || raw.contains('+') {
        return None;
    }
    let mut parts = raw.split('.');
    let version = StableVersion(
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() {
        None
    } else {
        Some(version)
    }
}

fn release_is_newer(release: &Release, current: StableVersion) -> bool {
    let Some(latest) = parse_stable_version(&release.tag_name) else {
        return false;
    };
    latest > current && !(current.0 == 0 && latest.0 > 0)
}

fn go_timestamp(time: OffsetDateTime) -> String {
    let rendered = time.format(&Rfc3339).unwrap_or_default();
    rendered
        .split_once('.')
        .map_or(rendered.clone(), |(seconds, _)| format!("{seconds}Z"))
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
