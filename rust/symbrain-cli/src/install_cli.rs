use chrono::Utc;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use symbrain_core::exit;
use symbrain_core::xdg;
use symbrain_harness::{
    AtomicFile, ConfigLocation, Entry, Harness, HarnessError, MAX_CONFIG_BYTES, SERVER_NAME,
};

const INSTALL_USAGE: &str = "Usage of install:\n  -dry-run\n    \tprint a unified diff of the change and write nothing\n  -harness string\n    \tharness to install into: claude, claude-desktop, cursor, opencode, codex, antigravity (required)\n  -keep-superseded\n    \tkeep superseded symmemory/symskills MCP entries instead of migrating them out\n  -profile string\n    \tprofile to bind this harness connection to (default: the global config's default_profile)\n  -project string\n    \tproject directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)\n";

const UNINSTALL_USAGE: &str = "Usage of uninstall:\n  -dry-run\n    \tprint a unified diff of the change and write nothing\n  -harness string\n    \tharness to remove symbrain from: claude, claude-desktop, cursor, opencode, codex, antigravity (required)\n  -project string\n    \tproject directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)\n";

#[derive(Default)]
struct InstallArgs {
    harness: String,
    profile: String,
    project: OsString,
    dry_run: bool,
    keep_superseded: bool,
}

#[derive(Default)]
struct UninstallArgs {
    harness: String,
    project: OsString,
    dry_run: bool,
}

struct ResolvedTarget {
    harness: &'static Harness,
    location: ConfigLocation,
    /// The lexically cleaned `--project` directory. Go resolves it with plain
    /// `os` calls, so the operating system follows its symlinks.
    project_root: Option<PathBuf>,
}

pub(crate) fn run_install(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let Some(parsed) = parse_install_args(args, stderr) else {
        return exit::USAGE;
    };
    let Some(target) = resolve_harness(&parsed.harness, &parsed.project, "install", stderr) else {
        return exit::USAGE;
    };

    let profile = if parsed.profile.is_empty() {
        match default_profile() {
            Ok(profile) => profile,
            Err(error) => {
                let _ = writeln!(stderr, "symbrain install: {error}");
                return exit::USAGE;
            }
        }
    } else {
        parsed.profile
    };
    if profile.is_empty() {
        let _ = writeln!(
            stderr,
            "symbrain install: no --profile given and no default_profile configured in ~/.config/symbrain/config.toml (pass --profile, or run `symbrain init`)"
        );
        return exit::NO_INPUT;
    }

    install_into(
        &target,
        &profile,
        parsed.dry_run,
        parsed.keep_superseded,
        stdout,
        stderr,
    )
}

pub(crate) fn run_uninstall(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let Some(parsed) = parse_uninstall_args(args, stderr) else {
        return exit::USAGE;
    };
    let Some(target) = resolve_harness(&parsed.harness, &parsed.project, "uninstall", stderr)
    else {
        return exit::USAGE;
    };
    uninstall_from(&target, parsed.dry_run, stdout, stderr)
}

fn parse_install_args(args: &[OsString], stderr: &mut dyn Write) -> Option<InstallArgs> {
    let mut parsed = InstallArgs::default();
    let normalized = crate::normalize_flags(args);
    if !parse_flags(&normalized, &mut parsed, stderr, INSTALL_USAGE) {
        return None;
    }
    Some(parsed)
}

fn parse_uninstall_args(args: &[OsString], stderr: &mut dyn Write) -> Option<UninstallArgs> {
    let mut parsed = UninstallArgs::default();
    let normalized = crate::normalize_flags(args);
    if !parse_flags(&normalized, &mut parsed, stderr, UNINSTALL_USAGE) {
        return None;
    }
    Some(parsed)
}

trait FlagValues {
    fn set_value(&mut self, name: &str, value: OsString) -> bool;
    fn set_bool(&mut self, name: &str, value: bool) -> bool;
    fn has_value_flag(name: &str) -> bool;
    fn has_bool_flag(name: &str) -> bool;
}

impl FlagValues for InstallArgs {
    fn set_value(&mut self, name: &str, value: OsString) -> bool {
        match name {
            "harness" => self.harness = value.to_string_lossy().into_owned(),
            "profile" => self.profile = value.to_string_lossy().into_owned(),
            "project" => self.project = value,
            _ => return false,
        }
        true
    }

    fn set_bool(&mut self, name: &str, value: bool) -> bool {
        match name {
            "dry-run" => self.dry_run = value,
            "keep-superseded" => self.keep_superseded = value,
            _ => return false,
        }
        true
    }

    fn has_value_flag(name: &str) -> bool {
        matches!(name, "harness" | "profile" | "project")
    }

    fn has_bool_flag(name: &str) -> bool {
        matches!(name, "dry-run" | "keep-superseded")
    }
}

impl FlagValues for UninstallArgs {
    fn set_value(&mut self, name: &str, value: OsString) -> bool {
        match name {
            "harness" => self.harness = value.to_string_lossy().into_owned(),
            "project" => self.project = value,
            _ => return false,
        }
        true
    }

    fn set_bool(&mut self, name: &str, value: bool) -> bool {
        if name == "dry-run" {
            self.dry_run = value;
            true
        } else {
            false
        }
    }

    fn has_value_flag(name: &str) -> bool {
        matches!(name, "harness" | "project")
    }

    fn has_bool_flag(name: &str) -> bool {
        name == "dry-run"
    }
}

/// The raw bytes of `arg` after its first `skip` bytes.
fn os_suffix(arg: &OsString, skip: usize) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        OsString::from_vec(arg.as_os_str().as_bytes()[skip..].to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(arg.to_string_lossy()[skip..].to_owned())
    }
}

fn parse_flags<T: FlagValues>(
    args: &[OsString],
    parsed: &mut T,
    stderr: &mut dyn Write,
    usage: &str,
) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" || !arg.starts_with('-') || arg == "-" {
            // The Go flag package stops parsing at the first positional. The
            // command historically ignores those remaining positionals.
            break;
        }
        let name = arg.trim_start_matches('-');
        let dashes = arg.len() - name.len();
        let (name, inline_value) = name
            .split_once('=')
            .map_or((name, None), |(name, value)| (name, Some(value)));
        if name == "h" || name == "help" {
            let _ = write!(stderr, "{usage}");
            return false;
        }
        if T::has_value_flag(name) {
            let value = if inline_value.is_some() {
                // Raw bytes after `-name=`: a project path need not be UTF-8.
                os_suffix(&args[index], dashes + name.len() + 1)
            } else if let Some(value) = args.get(index + 1) {
                index += 1;
                value.clone()
            } else {
                let _ = writeln!(stderr, "flag needs an argument: -{name}");
                let _ = write!(stderr, "{usage}");
                return false;
            };
            let _ = parsed.set_value(name, value);
        } else if T::has_bool_flag(name) {
            let value = match inline_value {
                None | Some("true") => true,
                Some("false") => false,
                Some(value) => {
                    let _ = writeln!(
                        stderr,
                        "invalid boolean value {value:?} for -{name}: parse error"
                    );
                    let _ = write!(stderr, "{usage}");
                    return false;
                }
            };
            let _ = parsed.set_bool(name, value);
        } else {
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = write!(stderr, "{usage}");
            return false;
        }
        index += 1;
    }
    true
}

fn resolve_harness(
    name: &str,
    project: &std::ffi::OsStr,
    command: &str,
    stderr: &mut dyn Write,
) -> Option<ResolvedTarget> {
    if name.is_empty() {
        let names = symbrain_harness::names().join(", ");
        let _ = writeln!(
            stderr,
            "symbrain {command}: --harness is required (want one of: {names})"
        );
        return None;
    }
    let harness = match symbrain_harness::lookup(name) {
        Ok(harness) => harness,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain {command}: {error}");
            return None;
        }
    };
    if !harness.supports_mcp_install() {
        let _ = writeln!(
            stderr,
            "symbrain {command}: resolve config path for {}: harness does not support MCP installation",
            harness.name
        );
        return None;
    }
    if !project.is_empty() {
        if !harness.supports_project {
            let _ = writeln!(
                stderr,
                "symbrain {command}: harness {:?} has no project-local config; omit --project",
                harness.name.as_str()
            );
            return None;
        }
        let path = harness.project_config_path(Path::new(project))?;
        // Go joins and lexically cleans the project path, so `..` is folded
        // away before the operating system ever sees it.
        let root = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        return Some(ResolvedTarget {
            harness,
            location: ConfigLocation {
                path,
                trusted_root: root.clone(),
                relative_path: PathBuf::from(".mcp.json"),
            },
            project_root: Some(root),
        });
    }
    match harness.config_location() {
        Ok(location) => Some(ResolvedTarget {
            harness,
            location,
            project_root: None,
        }),
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain {command}: resolve config path for {}: {error}",
                harness.name
            );
            None
        }
    }
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_CONFIG_BYTES.min(64 * 1024));
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration file exceeds maximum size",
        ));
    }
    Ok(bytes)
}

fn default_profile() -> Result<String, HarnessError> {
    match std::env::var("SYMBRAIN_DEFAULT_PROFILE") {
        Ok(profile) if !profile.is_empty() => return Ok(profile),
        _ => {}
    }
    let path = xdg::config_path();
    let data = match read_bounded(&path) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(HarnessError::Io(error)),
    };
    let text = std::str::from_utf8(&data).map_err(|_| {
        HarnessError::Unsupported(format!(
            "config: failed to load {}: global config error: failed to parse {}: input is not UTF-8",
            path.display(),
            path.display()
        ))
    })?;
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        let detail = if error.message() == "unclosed array, expected `]`" {
            "toml: line 1 (last key \"invalid\"): expected value but found \"unterminated\" instead"
                .to_owned()
        } else {
            error.message().to_owned()
        };
        HarnessError::Unsupported(format!(
            "config: failed to load {}: global config error: failed to parse {}: {detail}",
            path.display(),
            path.display(),
        ))
    })?;
    Ok(document
        .get("default_profile")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or_default()
        .to_owned())
}

/// A byte-exact message part: paths keep their raw bytes, text is UTF-8.
trait Part {
    fn part(&self) -> Vec<u8>;
}

impl Part for str {
    fn part(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl Part for String {
    fn part(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl Part for Path {
    fn part(&self) -> Vec<u8> {
        path_bytes(self)
    }
}

impl Part for PathBuf {
    fn part(&self) -> Vec<u8> {
        path_bytes(self)
    }
}

impl<T: Part + ?Sized> Part for &T {
    fn part(&self) -> Vec<u8> {
        (**self).part()
    }
}

/// Any `Display` value (errors, harness names).
struct Shown<T>(T);

impl<T: std::fmt::Display> Part for Shown<T> {
    fn part(&self) -> Vec<u8> {
        self.0.to_string().into_bytes()
    }
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned().into_bytes()
    }
}

/// Writes the concatenated parts and a newline in one call.
macro_rules! say {
    ($writer:expr, $($part:expr),+ $(,)?) => {{
        let mut line: Vec<u8> = Vec::new();
        $( line.extend_from_slice(&Part::part(&$part)); )+
        line.push(b'\n');
        let _ = $writer.write_all(&line);
    }};
}

/// `symbrain COMMAND: read PATH: open PATH: errno` when the failure carries an
/// operating-system error (Go's `os.ReadFile`), otherwise the Rust wording.
fn say_read_failure(stderr: &mut dyn Write, command: &str, path: &Path, error: &io::Error) {
    if error.raw_os_error().is_some() {
        say!(
            stderr,
            "symbrain ",
            command,
            ": read ",
            path,
            ": open ",
            path,
            ": ",
            Shown(symbrain_instructions::go_io_text(error))
        );
    } else {
        say!(
            stderr,
            "symbrain ",
            command,
            ": read ",
            path,
            ": ",
            Shown(error)
        );
    }
}

/// Stands in for a non-UTF-8 path in a diff label until the bytes are spliced in.
const PLACEHOLDER: &str = "\u{1}symbrain-diff-label\u{1}";

/// The diff Go prints, with the label kept as raw bytes.
fn diff_bytes(path: &Path, old: &[u8], new: &[u8]) -> Vec<u8> {
    if let Some(text) = path.to_str() {
        return symbrain_harness::unified_diff(text, old, new).into_bytes();
    }
    let diff = symbrain_harness::unified_diff(PLACEHOLDER, old, new);
    let label = path_bytes(path);
    let mut out = Vec::with_capacity(diff.len() + label.len());
    let mut rest = diff.as_bytes();
    while let Some(at) = rest
        .windows(PLACEHOLDER.len())
        .position(|window| window == PLACEHOLDER.as_bytes())
    {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(&label);
        rest = &rest[at + PLACEHOLDER.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// `ENOTDIR` is 20 on every Unix this CLI supports (Linux, macOS, the BSDs).
#[cfg(unix)]
const ENOTDIR: i32 = 20;

/// Go's `os.MkdirAll(path, 0o700)`, including which component an error names.
#[cfg(unix)]
fn go_mkdir_all(path: &Path) -> Result<(), (PathBuf, io::Error)> {
    use std::os::unix::fs::DirBuilderExt;

    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => {
            return Err((path.to_path_buf(), io::Error::from_raw_os_error(ENOTDIR)));
        }
        Err(_) => {}
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && parent != Path::new("/")
    {
        go_mkdir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) => match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => Ok(()),
            _ => Err((path.to_path_buf(), error)),
        },
    }
}

#[cfg(not(unix))]
fn go_mkdir_all(path: &Path) -> Result<(), (PathBuf, io::Error)> {
    fs::create_dir_all(path).map_err(|error| (path.to_path_buf(), error))
}

/// The directory the operating system resolves a cleaned project root to,
/// following symlinks like Go's plain `os` calls. Windows keeps the cleaned
/// path (a verbatim `\\?\` prefix would change the capability anchor).
#[cfg(unix)]
fn physical_root(root: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(root)
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // same signature as the fallible Unix variant
fn physical_root(root: &Path) -> io::Result<PathBuf> {
    Ok(root.to_path_buf())
}

/// Opens the existing config through the retained capability. A project root
/// is resolved like Go's `os.ReadFile` does: lexically cleaned, symlinks
/// followed by the operating system.
fn open_existing(
    target: &ResolvedTarget,
) -> io::Result<(Option<AtomicFile>, Option<symbrain_harness::FileSnapshot>)> {
    let location = &target.location;
    let trusted_root = match &target.project_root {
        Some(root) => match physical_root(root) {
            Ok(physical) => physical,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((None, None)),
            Err(error) => return Err(error),
        },
        None => location.trusted_root.clone(),
    };
    let capability = match AtomicFile::open(
        &trusted_root,
        &location.relative_path,
        location.path.clone(),
        false,
    ) {
        Ok(capability) => capability,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => return Err(error),
    };
    match capability.read_snapshot() {
        Ok(snapshot) => Ok((Some(capability), Some(snapshot))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok((None, None)),
        Err(error) => Err(error),
    }
}

/// Creates the missing project directories like Go's `os.MkdirAll` and opens
/// the target for writing.
fn open_for_create(
    command: &str,
    target: &ResolvedTarget,
    stderr: &mut dyn Write,
) -> Option<AtomicFile> {
    let location = &target.location;
    let trusted_root = match &target.project_root {
        Some(root) => {
            if let Err((failed, error)) = go_mkdir_all(root) {
                say!(
                    stderr,
                    "symbrain ",
                    command,
                    ": create ",
                    root,
                    ": mkdir ",
                    failed,
                    ": ",
                    Shown(symbrain_instructions::go_io_text(&error))
                );
                return None;
            }
            physical_root(root).unwrap_or_else(|_| root.clone())
        }
        None => location.trusted_root.clone(),
    };
    match AtomicFile::open(
        &trusted_root,
        &location.relative_path,
        location.path.clone(),
        target.project_root.is_none(),
    ) {
        Ok(capability) => Some(capability),
        Err(error) => {
            say!(
                stderr,
                "symbrain ",
                command,
                ": create ",
                location.path,
                ": ",
                Shown(error)
            );
            None
        }
    }
}

#[allow(clippy::too_many_lines)]
fn install_into(
    target: &ResolvedTarget,
    profile: &str,
    dry_run: bool,
    keep_superseded: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let harness = target.harness;
    let location = &target.location;
    let (capability, snapshot) = match open_existing(target) {
        Ok(value) => value,
        Err(error) => {
            say_read_failure(stderr, "install", &location.path, &error);
            return exit::GENERIC;
        }
    };
    let (original, existed) = snapshot.as_ref().map_or_else(
        || (Vec::new(), false),
        |snapshot| (snapshot.bytes.clone(), true),
    );
    let mut document = if existed {
        match symbrain_harness::parse(harness, &original) {
            Ok(document) => document,
            Err(error) => {
                say!(
                    stderr,
                    "symbrain install: ",
                    location.path,
                    ": ",
                    format_parse_error(harness, &error)
                );
                return exit::NO_INPUT;
            }
        }
    } else {
        symbrain_harness::empty(harness)
    };

    let mut migrated = Vec::new();
    if !keep_superseded {
        for name in document.server_names() {
            let Some(entry) = document.server(&name) else {
                continue;
            };
            let Some(core) = entry.superseded_core() else {
                continue;
            };
            if document.remove_server(&name) {
                migrated.push(format!("{name} (superseded {core})"));
            }
        }
    }
    document.set_server(SERVER_NAME, Entry::new(profile));
    let new_content = match document.marshal() {
        Ok(content) => content,
        Err(error) => {
            say!(
                stderr,
                "symbrain install: encode ",
                location.path,
                ": ",
                Shown(error)
            );
            return exit::GENERIC;
        }
    };

    if dry_run {
        let diff = diff_bytes(&location.path, &original, &new_content);
        if diff.is_empty() {
            say!(
                stdout,
                location.path,
                ": already up to date, no changes to make"
            );
        } else {
            let _ = stdout.write_all(&diff);
            if !migrated.is_empty() {
                say!(
                    stdout,
                    "migrated superseded core entries: ",
                    migrated.join(", ")
                );
            }
        }
        return exit::OK;
    }

    let capability = match capability {
        Some(capability) => capability,
        None => match open_for_create("install", target, stderr) {
            Some(capability) => capability,
            None => return exit::GENERIC,
        },
    };
    if let (Some(snapshot), true) = (snapshot.as_ref(), existed) {
        match capability.backup_snapshot(snapshot, &Utc::now().format("%Y%m%dT%H%M%SZ").to_string())
        {
            Ok(backup) => {
                say!(stdout, "backed up ", location.path, " to ", backup);
                if !migrated.is_empty() {
                    say!(
                        stdout,
                        "migrated superseded core entries: ",
                        migrated.join(", "),
                        " (roll back by restoring ",
                        backup,
                        ")"
                    );
                }
            }
            Err(error) => {
                say!(
                    stderr,
                    "symbrain install: back up ",
                    location.path,
                    ": ",
                    Shown(error)
                );
                return exit::GENERIC;
            }
        }
    }

    if let Err(error) = capability.write(&new_content, 0o600) {
        say!(
            stderr,
            "symbrain install: write ",
            location.path,
            ": ",
            Shown(error)
        );
        return exit::GENERIC;
    }
    say!(
        stdout,
        "installed symbrain into ",
        location.path,
        " (harness: ",
        Shown(harness.name),
        ", profile: ",
        profile,
        ")"
    );
    exit::OK
}

#[allow(clippy::too_many_lines)]
fn uninstall_from(
    target: &ResolvedTarget,
    dry_run: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let harness = target.harness;
    let location = &target.location;
    let (capability, snapshot) = match open_existing(target) {
        Ok(value) => value,
        Err(error) => {
            say_read_failure(stderr, "uninstall", &location.path, &error);
            return exit::GENERIC;
        }
    };
    let Some(snapshot) = snapshot else {
        say!(
            stdout,
            location.path,
            ": no config file found, nothing to uninstall"
        );
        return exit::OK;
    };
    let original = snapshot.bytes.clone();
    let mut document = match symbrain_harness::parse(harness, &original) {
        Ok(document) => document,
        Err(error) => {
            say!(
                stderr,
                "symbrain uninstall: ",
                location.path,
                ": ",
                format_parse_error(harness, &error)
            );
            return exit::NO_INPUT;
        }
    };
    let Some(entry) = document.server(SERVER_NAME) else {
        say!(
            stdout,
            location.path,
            ": symbrain is not installed, nothing to do"
        );
        return exit::OK;
    };
    if !entry.is_symbrain() {
        say!(
            stdout,
            location.path,
            ": symbrain is not installed, nothing to do"
        );
        return exit::OK;
    }
    document.remove_server(SERVER_NAME);
    let new_content = match document.marshal() {
        Ok(content) => content,
        Err(error) => {
            say!(
                stderr,
                "symbrain uninstall: encode ",
                location.path,
                ": ",
                Shown(error)
            );
            return exit::GENERIC;
        }
    };
    if dry_run {
        let _ = stdout.write_all(&diff_bytes(&location.path, &original, &new_content));
        return exit::OK;
    }
    let Some(capability) = capability else {
        return exit::GENERIC;
    };
    match capability.backup_snapshot(&snapshot, &Utc::now().format("%Y%m%dT%H%M%SZ").to_string()) {
        Ok(backup) => {
            say!(stdout, "backed up ", location.path, " to ", backup);
        }
        Err(error) => {
            say!(
                stderr,
                "symbrain uninstall: back up ",
                location.path,
                ": ",
                Shown(error)
            );
            return exit::GENERIC;
        }
    }
    if let Err(error) = capability.write(&new_content, 0o600) {
        say!(
            stderr,
            "symbrain uninstall: write ",
            location.path,
            ": ",
            Shown(error)
        );
        return exit::GENERIC;
    }
    say!(
        stdout,
        "removed symbrain from ",
        location.path,
        " (harness: ",
        Shown(harness.name),
        ")"
    );
    exit::OK
}

fn format_parse_error(harness: &Harness, error: &HarnessError) -> String {
    let detail = match error {
        HarnessError::Json(message) | HarnessError::Toml(message) => message.clone(),
        _ => error.to_string(),
    };
    let format = harness.format.to_string();
    format!(
        "harness: {} config is not valid {format}; refusing to edit a config symbrain cannot parse: parse {format}: {detail}",
        harness.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_all_install_flags() {
        let args = [
            OsString::from("--harness"),
            OsString::from("claude"),
            OsString::from("--profile=p"),
            OsString::from("--project"),
            OsString::from("/tmp/project"),
            OsString::from("--dry-run"),
            OsString::from("--keep-superseded"),
        ];
        let mut stderr = Vec::new();
        let parsed = parse_install_args(&args, &mut stderr).unwrap();
        assert_eq!(parsed.harness, "claude");
        assert_eq!(parsed.profile, "p");
        assert_eq!(parsed.project, OsString::from("/tmp/project"));
        assert!(parsed.dry_run);
        assert!(parsed.keep_superseded);
        assert!(stderr.is_empty());
    }

    #[test]
    fn foreign_symbrain_entry_is_not_removed() {
        let harness = symbrain_harness::lookup("cursor").unwrap();
        let document = symbrain_harness::parse(
            harness,
            br#"{"mcpServers":{"symbrain":{"command":"other-tool","args":["x"]}}}"#,
        )
        .unwrap();
        let entry = document.server(SERVER_NAME).unwrap();
        assert!(!entry.is_symbrain());
        assert_eq!(document.server(SERVER_NAME), Some(entry));
    }

    #[test]
    fn superseded_names_are_exact_basenames() {
        assert_eq!(
            symbrain_harness::SUPERSEDED_CORE_NAMES,
            ["symmemory", "symskills"]
        );
        assert_eq!(Entry::new("p").command, SERVER_NAME);
        assert!(
            Entry {
                command: "/bin/symmemory".into(),
                args: vec![]
            }
            .superseded_core()
            .is_some()
        );
        assert!(
            Entry {
                command: "/bin/symvault".into(),
                args: vec![]
            }
            .superseded_core()
            .is_none()
        );
    }
}
