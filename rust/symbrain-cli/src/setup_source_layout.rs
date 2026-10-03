//! Preserve the source builder's macOS external-volume/cache ownership.
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct Layout {
    pub temp: Option<PathBuf>,
    pub base: Option<PathBuf>,
    pub swift_cache: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)] // The macOS implementation can fail before any build.
pub(super) fn prepare(_context: &super::process::Context) -> Result<Layout, String> {
    Ok(Layout::default())
}

#[cfg(target_os = "macos")]
pub(super) fn prepare(context: &super::process::Context) -> Result<Layout, String> {
    use super::process;
    use std::ffi::OsStr;
    use std::path::Path;
    use std::time::Duration;
    use symbrain_managed::format_io_error;
    const VOLUME: &str = "/Volumes/1TB_NVMe_SN850X";
    if std::env::var_os("CI").is_some_and(|value| !value.is_empty()) {
        return Ok(Layout::default());
    }
    let volume = std::fs::canonicalize(VOLUME).map_err(|error| {
        format!(
            "required external build volume {VOLUME} is unavailable: lstat {VOLUME}: {}",
            format_io_error(&error)
        )
    })?;
    if !volume.is_dir() {
        return Err(format!(
            "required external build volume {VOLUME} is unavailable: not a directory"
        ));
    }
    let df = process::lookup("df").ok().and_then(|df| {
        process::run(
            &df,
            &[OsStr::new("-P"), volume.as_os_str()],
            None,
            &[],
            false,
            Duration::from_secs(30),
            context,
        )
        .ok()
    });
    let mounted = df.filter(|out| out.error.is_none()).is_some_and(|out| {
        String::from_utf8_lossy(&out.bytes)
            .trim()
            .lines()
            .last()
            .and_then(|line| line.split_whitespace().last())
            .is_some_and(|path| Path::new(path) == volume)
    });
    if !mounted {
        return Err(format!(
            "required external build volume {VOLUME} is not mounted"
        ));
    }
    let base = std::env::var_os("SYMAIRA_EXTERNAL_BASE")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || PathBuf::from(VOLUME).join("Dev/Symaira_Dev/builds/symaira-brain"),
            PathBuf::from,
        );
    let runtime = std::env::var_os("SYMAIRA_EXTERNAL_RUNTIME_ROOT")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(VOLUME).join("tmp"), PathBuf::from);
    for (name, path) in [
        ("SYMAIRA_EXTERNAL_BASE", &base),
        ("SYMAIRA_EXTERNAL_RUNTIME_ROOT", &runtime),
    ] {
        validate(&volume, path).map_err(|error| format!("{name}: {error}"))?;
    }
    let temp = base.join("tmp");
    let swift_cache = base.join("swift-cache");
    let paths = [
        ("TMPDIR", temp.clone()),
        ("TMP", temp.clone()),
        ("TEMP", temp.clone()),
        ("GOTMPDIR", base.join("go-tmp")),
        ("GOPATH", base.join("gopath")),
        ("GOTELEMETRYDIR", base.join("go-telemetry")),
        ("GOCACHE", base.join("go-cache")),
        ("GOMODCACHE", base.join("go-mod-cache")),
        ("CARGO_HOME", base.join("cargo-home")),
        ("CARGO_TARGET_DIR", base.join("cargo-target")),
        ("PYTHONPYCACHEPREFIX", base.join("python-cache")),
        ("SWIFT_CACHE", swift_cache.clone()),
        ("SYMAIRA_EXTERNAL_RUNTIME_ROOT", runtime),
    ];
    for (_, path) in &paths {
        mkdir(path)
            .map_err(|error| format!("create external build path {}: {error}", path.display()))?;
        validate(&volume, path)
            .map_err(|error| format!("external build path {}: {error}", path.display()))?;
    }
    let mut environment: Vec<_> = paths
        .into_iter()
        .filter(|(name, _)| *name != "SWIFT_CACHE")
        .map(|(name, path)| (name.into(), path.into_os_string()))
        .collect();
    environment.push(("RUSTUP_NO_UPDATE_CHECK".into(), "1".into()));
    Ok(Layout {
        temp: Some(temp),
        base: Some(base),
        swift_cache,
        environment,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn mkdir(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| {
            format!(
                "mkdir {}: {}",
                path.display(),
                symbrain_managed::format_io_error(&error)
            )
        })
}

#[cfg(target_os = "macos")]
fn validate(volume: &std::path::Path, candidate: &std::path::Path) -> Result<(), String> {
    use std::path::Component;
    if !candidate.is_absolute() {
        return Err(format!("must be absolute and under {}", volume.display()));
    }
    let mut clean = PathBuf::new();
    for part in candidate.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    let mut parent = clean.as_path();
    loop {
        if parent.exists() {
            let resolved = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
            return if resolved.starts_with(volume) {
                Ok(())
            } else {
                Err(format!("resolves outside {}", volume.display()))
            };
        }
        parent = parent
            .parent()
            .ok_or_else(|| "cannot resolve path".to_string())?;
    }
}
