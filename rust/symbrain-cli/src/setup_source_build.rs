//! The Go/Swift toolchain argv contract for optional source workers.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Spec, layout, process};
use symbrain_managed::{GoText, format_io_error};

const TOOL_BUDGET: Duration = Duration::from_secs(30);
const BUILD_BUDGET: Duration = Duration::from_mins(30);

pub(super) fn receiver_commit(root: &Path, context: &process::Context) -> Result<String, GoText> {
    let git = process::lookup("git")
        .map_err(|error| GoText::from(error).with_prefix("resolve receiver commit: "))?;
    output(
        &git,
        &[
            OsStr::new("-C"),
            root.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("HEAD"),
        ],
        None,
        &[],
        false,
        TOOL_BUDGET,
        context,
    )
    .map(|out| String::from_utf8_lossy(&out).trim().into())
    .map_err(|error| error.with_prefix("resolve receiver commit: "))
}

pub(super) fn build(
    root: &Path,
    spec: Spec,
    dest: &Path,
    context: &process::Context,
) -> Result<(PathBuf, Vec<u8>), GoText> {
    if spec.module == "browse" {
        browse(root, dest, context)
    } else {
        swift(root, spec, dest, context)
    }
}

fn browse(
    root: &Path,
    dest: &Path,
    context: &process::Context,
) -> Result<(PathBuf, Vec<u8>), GoText> {
    let tool = process::lookup("go")
        .map_err(|error| format!("go toolchain not found on PATH: {error}"))?;
    let version = output(
        &tool,
        &[OsStr::new("version")],
        None,
        &[],
        false,
        TOOL_BUDGET,
        context,
    )
    .map_err(|error| error.with_prefix("go version: "))?;
    let builder = trim_identity(&version);
    let target = dest.join("symbrowse");
    let mut environment = layout::prepare(context)?.environment;
    environment.push(("CGO_ENABLED".into(), "0".into()));
    let args = [
        OsStr::new("build"),
        OsStr::new("-trimpath"),
        OsStr::new("-o"),
        target.as_os_str(),
        OsStr::new("./cmd/symbrowse"),
    ];
    output(
        &tool,
        &args,
        Some(&root.join("browse")),
        &environment,
        true,
        BUILD_BUDGET,
        context,
    )
    .map_err(|error| error.with_prefix("go build ./cmd/symbrowse: "))?;
    Ok((target, builder))
}

fn swift(
    root: &Path,
    spec: Spec,
    dest: &Path,
    context: &process::Context,
) -> Result<(PathBuf, Vec<u8>), GoText> {
    let tool = process::lookup("swift")
        .map_err(|error| format!("swift toolchain not found on PATH: {error}"))?;
    let version = output(
        &tool,
        &[OsStr::new("--version")],
        None,
        &[],
        false,
        TOOL_BUDGET,
        context,
    )
    .map_err(|error| error.with_prefix("swift --version: "))?;
    let version = trim_identity(&version);
    let builder = version
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default()
        .to_vec();
    let layout = layout::prepare(context)?;
    let pkg = root.join(spec.module);
    let mut args = vec![
        OsStr::new("build"),
        OsStr::new("--package-path"),
        pkg.as_os_str(),
    ];
    let scratch = layout
        .base
        .as_ref()
        .map(|base| base.join("swift-scratch").join(spec.module));
    #[cfg(target_os = "macos")]
    if let Some(scratch) = &scratch {
        layout::mkdir(scratch).map_err(|error| error.with_prefix("create Swift scratch path: "))?;
        args.extend([
            OsStr::new("--scratch-path"),
            scratch.as_os_str(),
            OsStr::new("--cache-path"),
            layout.swift_cache.as_os_str(),
        ]);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (&scratch, &layout.swift_cache);
    output(
        &tool,
        &args,
        None,
        &layout.environment,
        true,
        BUILD_BUDGET,
        context,
    )
    .map_err(|error| error.with_prefix(&format!("swift build ({}): ", spec.module)))?;
    args.push(OsStr::new("--show-bin-path"));
    let out = output(
        &tool,
        &args,
        None,
        &layout.environment,
        false,
        TOOL_BUDGET,
        context,
    )
    .map_err(|error| {
        error.with_prefix(&format!("swift build --show-bin-path ({}): ", spec.module))
    })?;
    #[cfg(unix)]
    let built = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(
            super::text::trim_space(&out).to_vec(),
        ))
    };
    #[cfg(not(unix))]
    let built = PathBuf::from(String::from_utf8_lossy(super::text::trim_space(&out)).as_ref());
    let built = built.join(spec.binary);
    std::fs::metadata(&built).map_err(|error| {
        let mut message = GoText::path("expected built binary ", &built, ": stat ");
        message.push(&symbrain_core::config::os_bytes(built.as_os_str()));
        message.with_suffix(format!(": {}", format_io_error(&error)).as_bytes())
    })?;
    let bytes = std::fs::read(&built).map_err(|error| {
        let mut message = GoText::path("read built binary ", &built, ": open ");
        message.push(&symbrain_core::config::os_bytes(built.as_os_str()));
        message.with_suffix(format!(": {}", format_io_error(&error)).as_bytes())
    })?;
    let target = dest.join(spec.binary);
    std::fs::write(&target, bytes).map_err(|error| {
        GoText::path(
            "stage built binary: open ",
            &target,
            &format!(": {}", format_io_error(&error)),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    Ok((target, builder))
}

#[allow(clippy::too_many_arguments)] // Mirrors the owned process inputs.
fn output(
    executable: &Path,
    args: &[&OsStr],
    cwd: Option<&Path>,
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
    combined: bool,
    budget: Duration,
    context: &process::Context,
) -> Result<Vec<u8>, GoText> {
    let out = process::run(
        executable,
        args,
        cwd,
        environment,
        combined,
        budget,
        context,
    )
    .map_err(|error| {
        if combined {
            error.with_suffix(b"\n")
        } else {
            error
        }
    })?;
    if let Some(error) = out.error {
        return Err(if combined {
            GoText::from(error)
                .with_suffix(b"\n")
                .with_suffix(&out.bytes)
        } else {
            error.into()
        });
    }
    Ok(out.bytes)
}

fn trim_identity(bytes: &[u8]) -> Vec<u8> {
    super::text::trim_space(bytes).to_vec()
}
