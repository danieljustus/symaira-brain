//! Native orchestration of explicitly selected Brain-owned source workers.
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use symbrain_core::exit;
use symbrain_managed::{
    SourceOrigin, format_io_error, install_source, installed_version, read_provenance,
};

#[path = "setup_source_build.rs"]
mod build;
#[path = "setup_source_layout.rs"]
mod layout;
#[path = "setup_source_process.rs"]
mod process;

#[derive(Clone, Copy)]
struct Spec {
    module: &'static str,
    binary: &'static str,
    darwin_only: bool,
}
const SPECS: [Spec; 3] = [
    Spec {
        module: "browse",
        binary: "symbrowse",
        darwin_only: false,
    },
    Spec {
        module: "operate",
        binary: "symoperate",
        darwin_only: true,
    },
    Spec {
        module: "scope",
        binary: "symscope",
        darwin_only: true,
    },
];

#[derive(Serialize)]
struct Report {
    bin_dir: String,
    root: String,
    results: Vec<ResultRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

#[derive(Serialize, Default)]
struct ResultRow {
    module: &'static str,
    binary: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    version: String,
    status: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    receiver_commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    binary_sha256: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

pub(super) fn valid_root(root: &OsStr) -> bool {
    absolute(root).is_ok_and(|root| root.is_dir())
}

fn absolute(root: &OsStr) -> Result<PathBuf, String> {
    let root = Path::new(root);
    let joined = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(root)
    };
    let mut clean = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}

fn select(modules: &str) -> Result<Vec<Spec>, String> {
    let mut wanted: BTreeSet<String> = if modules.is_empty() {
        let enabled = super::enabled_cores()?;
        SPECS
            .iter()
            .filter(|spec| enabled.get(spec.binary).copied().unwrap_or(false))
            .map(|spec| spec.module.into())
            .collect()
    } else {
        modules
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect()
    };
    if wanted.is_empty() {
        return Err("no modules selected: enable modules in config ([modules] browse/operate/scope) or pass --modules browse,operate,scope".into());
    }
    let mut specs = Vec::new();
    for spec in SPECS {
        if wanted.remove(spec.module) {
            specs.push(spec);
        }
    }
    if let Some(unknown) = wanted.first() {
        return Err(format!(
            "unknown module {} (known: browse, operate, scope)",
            symbrain_core::config::format_go_quoted(OsStr::new(unknown))
        ));
    }
    Ok(specs)
}

pub(super) fn run(
    bin_dir: &Path,
    root: &OsStr,
    modules: &str,
    json: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = match absolute(root) {
        Ok(root) if root.is_dir() => root,
        Ok(root) => {
            let _ = writeln!(
                stderr,
                "symbrain setup --from-source: {} is not a directory",
                root.display()
            );
            return exit::USAGE;
        }
        Err(error) => return failed(stderr, &error, exit::USAGE),
    };
    let specs = match select(modules) {
        Ok(specs) => specs,
        Err(error) => return failed(stderr, &error, exit::USAGE),
    };
    let context = match process::Context::new() {
        Ok(context) => context,
        Err(error) => return failed(stderr, &error, exit::GENERIC),
    };
    let layout = match layout::prepare(&context) {
        Ok(layout) => layout,
        Err(error) => return failed(stderr, &error, exit::GENERIC),
    };
    let commit = match build::receiver_commit(&root, &context) {
        Ok(commit) => commit,
        Err(error) => return failed(stderr, &error, exit::GENERIC),
    };
    let mut report = Report {
        bin_dir: bin_dir.display().to_string(),
        root: root.display().to_string(),
        results: Vec::new(),
        errors: Vec::new(),
    };
    for spec in specs {
        let mut result = ResultRow {
            module: spec.module,
            binary: spec.binary,
            ..ResultRow::default()
        };
        if spec.darwin_only && !cfg!(target_os = "macos") {
            result.status = "skipped";
            result.error = "unsupported platform (darwin only)".into();
            if !json {
                let _ = writeln!(stdout, "  -  {} (unsupported platform)", spec.binary);
            }
            report.results.push(result);
            continue;
        }
        match install(&root, spec, bin_dir, &commit, &layout, &context) {
            Ok((version, hash)) => {
                result.status = "installed";
                result.version = version;
                result.source = "brain-source".into();
                result.receiver_commit.clone_from(&commit);
                result.binary_sha256 = hash;
                if !json {
                    let short = commit.get(..12).unwrap_or(&commit);
                    let _ = writeln!(
                        stdout,
                        "  ✓  {} {} (brain-source, {short})",
                        spec.binary, result.version
                    );
                }
            }
            Err((error, build_error)) => {
                result.status = "error";
                // Go keeps the explanatory prefix in the row, but aggregates the underlying probe error.
                let aggregate = error
                    .strip_prefix("installed but version probe failed: ")
                    .unwrap_or(&error);
                report.errors.push(format!("{}: {aggregate}", spec.module));
                if !json && build_error {
                    let _ = writeln!(stderr, "  ✗  {}: {error}", spec.binary);
                }
                result.error = error;
            }
        }
        report.results.push(result);
    }
    if json {
        let bytes = serde_json::to_string(&report).map(|text| go_html_escape(&text));
        if let Err(error) = bytes
            .map_err(std::io::Error::other)
            .and_then(|text| writeln!(stdout, "{text}"))
        {
            return failed(stderr, &format!("encode JSON: {error}"), exit::GENERIC);
        }
    } else {
        let _ = writeln!(stdout, "\nInstalled to {}", bin_dir.display());
    }
    if report.errors.is_empty() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn install(
    root: &Path,
    spec: Spec,
    bin_dir: &Path,
    commit: &str,
    layout: &layout::Layout,
    context: &process::Context,
) -> Result<(String, String), (String, bool)> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("symbrain-source-build-");
    let temp = layout
        .temp
        .as_ref()
        .map_or_else(|| builder.tempdir(), |parent| builder.tempdir_in(parent))
        .map_err(|error| (format_io_error(&error), false))?;
    let (binary, builder) =
        build::build(root, spec, temp.path(), context).map_err(|error| (error, true))?;
    let data = std::fs::read(&binary).map_err(|error| {
        let operation = if error.kind() == std::io::ErrorKind::IsADirectory {
            "read"
        } else {
            "open"
        };
        (
            format!(
                "{operation} {}: {}",
                binary.display(),
                format_io_error(&error)
            ),
            false,
        )
    })?;
    install_source(
        bin_dir,
        spec.binary,
        &data,
        SourceOrigin {
            receiver_commit: commit,
            module_dir: spec.module,
            builder: &builder,
        },
    )
    .map_err(|error| (error.to_string(), false))?;
    drop(temp);
    let version = installed_version(bin_dir, spec.binary).map_err(|error| {
        (
            format!("installed but version probe failed: {error}"),
            false,
        )
    })?;
    let hash = read_provenance(bin_dir, spec.binary)
        .ok()
        .flatten()
        .map_or_else(String::new, |record| record.binary_sha256);
    Ok((version, hash))
}

fn failed(stderr: &mut dyn Write, error: &str, code: u8) -> u8 {
    let _ = writeln!(stderr, "symbrain setup --from-source: {error}");
    code
}

fn go_html_escape(text: &str) -> String {
    text.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
