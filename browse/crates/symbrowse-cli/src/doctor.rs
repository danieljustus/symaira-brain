#![deny(unsafe_code)]

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use symbrowse_core::{
    config::{FlagOverrides, LoadContext, load},
    error::ErrorCode,
    key_resolver::KeyResolver,
    key_sources::SystemKeySources,
    output::{Envelope, Format},
};

const PASS: &str = "pass";
const WARN: &str = "warn";
const FAIL: &str = "fail";
const SKIPPED: &str = "skipped";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const CDP_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_PROBE_OUTPUT: usize = 1 << 20;
static NEXT_PROBE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
struct Check {
    name: String,
    status: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Serialize)]
struct Report {
    status: String,
    checks: Vec<Check>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixes: Vec<String>,
}

impl Check {
    fn new(name: &str, status: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            status: status.to_owned(),
            message: message.into(),
            details: None,
        }
    }

    fn details(mut self, values: impl IntoIterator<Item = (String, String)>) -> Self {
        self.details = Some(values.into_iter().collect());
        self
    }
}

pub fn run(format: Format, fix: bool) -> ExitCode {
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => return write_config_error(format, error.to_string()),
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => return write_config_error(format, error.to_string()),
    };

    // The Go CLI currently constructs Options without Engine or CDPEndpoint,
    // so doctor reports Chrome and reads only SYMBROWSE_CDP_ENDPOINT here.
    let mut checks = Vec::with_capacity(10);
    checks.push(check_engine());
    let (chrome, browser_path) = check_browser(&config.executable_path);
    let chrome_failed = chrome.status == FAIL;
    checks.push(chrome);
    checks.push(browser_path.map_or_else(
        || {
            Check::new(
                "version",
                SKIPPED,
                "skipped because no browser executable was found",
            )
        },
        |path| check_version(&path),
    ));
    let cdp_endpoint = env::var("SYMBROWSE_CDP_ENDPOINT").unwrap_or_default();
    checks.push(if cdp_endpoint.is_empty() {
        Check::new(
            "cdp",
            SKIPPED,
            "not checked: no CDP endpoint configured; daemon sessions launch their own browser (set SYMBROWSE_CDP_ENDPOINT or --cdp-endpoint to attach instead)",
        )
    } else {
        check_cdp(&cdp_endpoint)
    });
    checks.push(check_writable("config_dir", Path::new(&config.config_dir)));
    checks.push(check_writable("cache_dir", Path::new(&config.cache_dir)));
    checks.push(check_writable("state_dir", Path::new(&config.state_dir)));
    checks.push(check_writable("socket_dir", &socket_dir()));
    checks.push(check_key_source());

    // Go computes the report status before the CLI appends its guard check.
    let status = if checks.iter().any(|check| check.status == FAIL) {
        FAIL
    } else if checks.iter().any(|check| check.status == WARN) {
        WARN
    } else {
        PASS
    };
    checks.push(check_guard());
    let report = Report {
        status: status.to_owned(),
        checks,
        fixes: if fix {
            fixes(&config.executable_path)
        } else {
            Vec::new()
        },
    };
    if let Err(error) = write_report(&report, format) {
        let _ = writeln!(io::stderr(), "{error}");
        return ExitCode::from(1);
    }

    if report.status == FAIL {
        let (code, message) = if chrome_failed {
            (
                ErrorCode::NotFound,
                report
                    .checks
                    .iter()
                    .find(|check| check.name == "chrome" && check.status == FAIL)
                    .map(|check| check.message.as_str())
                    .unwrap_or("doctor check failed"),
            )
        } else {
            (
                ErrorCode::OperationFailed,
                "one or more doctor checks failed",
            )
        };
        return write_failure(format, code, message);
    }
    ExitCode::SUCCESS
}

fn write_config_error(format: Format, message: String) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else if let Ok(output) = Envelope::failure(ErrorCode::Config, message).render(format) {
        let _ = io::stdout().write_all(output.as_bytes());
    }
    ExitCode::from(ErrorCode::Config.exit_code())
}

fn write_report(report: &Report, format: Format) -> Result<(), String> {
    if format == Format::Text {
        for check in &report.checks {
            writeln!(
                io::stdout(),
                "[{}] {}: {}",
                check.status.to_uppercase(),
                check.name,
                check.message
            )
            .map_err(|error| error.to_string())?;
        }
        if !report.fixes.is_empty() {
            writeln!(io::stdout(), "\nCopyable next steps:").map_err(|error| error.to_string())?;
            for fix in &report.fixes {
                writeln!(io::stdout(), "- {fix}").map_err(|error| error.to_string())?;
            }
        }
        return Ok(());
    }
    if format == Format::Json {
        // Keep the Go envelope and report field order. Serializing through
        // serde_json::Value sorts object keys and changes the CLI bytes.
        #[derive(Serialize)]
        struct DoctorJson<'a> {
            success: bool,
            data: &'a Report,
        }

        let mut output = serde_json::to_string(&DoctorJson {
            success: true,
            data: report,
        })
        .map_err(|error| error.to_string())?;
        output.push('\n');
        return io::stdout()
            .write_all(output.as_bytes())
            .map_err(|error| error.to_string());
    }
    let data = serde_json::to_value(report).map_err(|error| error.to_string())?;
    let output = Envelope::ok(data, Vec::new())
        .render(format)
        .map_err(|error| error.to_string())?;
    io::stdout()
        .write_all(output.as_bytes())
        .map_err(|error| error.to_string())
}

fn write_failure(format: Format, code: ErrorCode, message: &str) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else if let Ok(output) = Envelope::failure(code, message).render(format) {
        let _ = io::stdout().write_all(output.as_bytes());
    }
    ExitCode::from(code.exit_code())
}

fn check_engine() -> Check {
    let caps = symbrowse_engine_chrome::canonical_capabilities();
    // The Go doctor constructs the default Chrome engine in launch mode.
    // Rust reports only interfaces backed by implemented daemon routes, but
    // its doctor must still describe the same default engine mode.
    let launch_mode = "launch";
    Check::new(
        "engine",
        PASS,
        format!(
            "engine {:?} ({}): {} optional interface(s) implemented, {} unsupported",
            caps.kind,
            launch_mode,
            caps.interfaces.len(),
            caps.unsupported.len()
        ),
    )
    .details([
        ("kind".to_owned(), caps.kind),
        ("launch_mode".to_owned(), launch_mode.to_owned()),
        ("interfaces".to_owned(), caps.interfaces.join(",")),
        ("unsupported".to_owned(), caps.unsupported.join(",")),
    ])
}

fn check_browser(override_path: &str) -> (Check, Option<PathBuf>) {
    let paths = browser_paths();
    let search = search_paths(&paths);
    if !override_path.is_empty() {
        if let Some(path) = resolve_executable(override_path) {
            return (
                Check::new(
                    "chrome",
                    PASS,
                    format!("using {} (SYMBROWSE_EXECUTABLE_PATH)", path.display()),
                )
                .details([
                    ("path".to_owned(), path.display().to_string()),
                    ("source".to_owned(), "SYMBROWSE_EXECUTABLE_PATH".to_owned()),
                ]),
                Some(path),
            );
        }
        return (
            Check::new(
                "chrome",
                FAIL,
                format!(
                    "Chrome, Chromium, or Edge was not found; searched {}; set SYMBROWSE_EXECUTABLE_PATH to override discovery",
                    search.join(", ")
                ),
            )
            .details([
                ("override".to_owned(), "SYMBROWSE_EXECUTABLE_PATH".to_owned()),
                ("search_paths".to_owned(), search.join("\n")),
            ]),
            None,
        );
    }
    for candidate in paths {
        if usable_executable(&candidate) {
            return (
                Check::new(
                    "chrome",
                    PASS,
                    format!("using {} (platform path)", candidate.display()),
                )
                .details([
                    ("path".to_owned(), candidate.display().to_string()),
                    ("source".to_owned(), "platform path".to_owned()),
                ]),
                Some(candidate),
            );
        }
    }
    for name in browser_names() {
        if let Some(path) = find_in_path(name)
            && usable_executable(&path)
        {
            return (
                Check::new("chrome", PASS, format!("using {} (PATH)", path.display())).details([
                    ("path".to_owned(), path.display().to_string()),
                    ("source".to_owned(), "PATH".to_owned()),
                ]),
                Some(path),
            );
        }
    }
    (
        Check::new(
            "chrome",
            FAIL,
            format!(
                "Chrome, Chromium, or Edge was not found; searched {}; set SYMBROWSE_EXECUTABLE_PATH to override discovery",
                search.join(", ")
            ),
        )
        .details([
            ("override".to_owned(), "SYMBROWSE_EXECUTABLE_PATH".to_owned()),
            ("search_paths".to_owned(), search.join("\n")),
        ]),
        None,
    )
}

fn browser_paths() -> Vec<PathBuf> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        [
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            home.join("Applications/Chromium.app/Contents/MacOS/Chromium"),
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            home.join("Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        ]
        .into()
    } else if cfg!(windows) {
        let program_files = env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        let x86 = env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        let local = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"));
        [
            program_files.join("Google/Chrome/Application/chrome.exe"),
            x86.join("Google/Chrome/Application/chrome.exe"),
            local.join("Google/Chrome/Application/chrome.exe"),
            program_files.join("Microsoft/Edge/Application/msedge.exe"),
            x86.join("Microsoft/Edge/Application/msedge.exe"),
            local.join("Microsoft/Edge/Application/msedge.exe"),
            program_files.join("Chromium/Application/chromium.exe"),
            local.join("Chromium/Application/chromium.exe"),
        ]
        .into()
    } else {
        [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/usr/bin/microsoft-edge-stable",
            "/opt/google/chrome/google-chrome",
            "/opt/microsoft/msedge/msedge",
            "/snap/bin/chromium",
        ]
        .map(PathBuf::from)
        .into()
    }
}

fn browser_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "chrome.exe",
            "chromium.exe",
            "msedge.exe",
            "chrome",
            "chromium",
            "microsoft-edge",
        ]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
        ]
    }
}

fn search_paths(paths: &[PathBuf]) -> Vec<String> {
    let mut values = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    values.push(format!("PATH: {}", browser_names().join(", ")));
    values
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

fn resolve_executable(value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        usable_executable(&path).then_some(path)
    } else {
        find_in_path(value).filter(|path| usable_executable(path))
    }
}

fn usable_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn check_version(path: &Path) -> Check {
    match run_bounded(path, &["--version"], COMMAND_TIMEOUT) {
        Err(CommandFailure::Timeout) => {
            Check::new("version", WARN, "version check timed out after 5s")
                .details([("path".to_owned(), path.display().to_string())])
        }
        Err(CommandFailure::Start(error)) => {
            Check::new("version", WARN, format!("version check failed: {error}"))
                .details([("path".to_owned(), path.display().to_string())])
        }
        Ok(output) if !output.status.success() => Check::new(
            "version",
            WARN,
            format!("version check failed: {}", output.status),
        )
        .details([("path".to_owned(), path.display().to_string())]),
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr)
            } else {
                String::from_utf8_lossy(&output.stdout)
            };
            let version = text.lines().next().unwrap_or_default().trim();
            if version.is_empty() {
                Check::new("version", WARN, "browser returned an empty version")
                    .details([("path".to_owned(), path.display().to_string())])
            } else {
                Check::new("version", PASS, version).details([
                    ("path".to_owned(), path.display().to_string()),
                    ("version".to_owned(), version.to_owned()),
                ])
            }
        }
    }
}

struct CapturedOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: ExitStatus,
}

enum CommandFailure {
    Timeout,
    Start(String),
}

fn run_bounded(
    path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CapturedOutput, CommandFailure> {
    let mut child = Command::new(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CommandFailure::Start(error.to_string()))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || read_probe_output(&mut stdout));
    let stderr_reader = thread::spawn(move || read_probe_output(&mut stderr));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(CommandFailure::Timeout);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(CommandFailure::Start(error.to_string()));
            }
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(CapturedOutput {
        stdout,
        stderr,
        status,
    })
}

fn read_probe_output(reader: &mut impl Read) -> Vec<u8> {
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 8192];
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let available = MAX_PROBE_OUTPUT.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..count.min(available)]);
    }
    retained
}

fn check_cdp(endpoint: &str) -> Check {
    let request = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let result = request
        .map_err(|error| error.to_string())
        .and_then(|runtime| {
            runtime.block_on(async {
                let client = reqwest::Client::builder()
                    .timeout(CDP_TIMEOUT)
                    .build()
                    .map_err(|e| e.to_string())?;
                let response = client
                    .get(endpoint)
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>((response.status().as_u16(), response.status().to_string()))
            })
        });
    match result {
        Err(error) if endpoint.starts_with("://") => {
            Check::new("cdp", WARN, format!("CDP endpoint is invalid: {error}"))
                .details([("endpoint".to_owned(), endpoint.to_owned())])
        }
        Err(error) => Check::new(
            "cdp",
            WARN,
            format!("CDP is not running or unreachable: {error}"),
        )
        .details([("endpoint".to_owned(), endpoint.to_owned())]),
        Ok((status, status_text)) if !(200..300).contains(&status) => Check::new(
            "cdp",
            WARN,
            format!("CDP endpoint returned HTTP {status_text}"),
        )
        .details([("endpoint".to_owned(), endpoint.to_owned())]),
        Ok(_) => Check::new(
            "cdp",
            PASS,
            "CDP endpoint is reachable; the daemon would attach to this browser (issue #296)",
        )
        .details([
            ("endpoint".to_owned(), endpoint.to_owned()),
            ("mode".to_owned(), "attach".to_owned()),
        ]),
    }
}

fn check_writable(name: &str, path: &Path) -> Check {
    if path.as_os_str().is_empty() {
        return Check::new(name, FAIL, "path is empty");
    }
    let mut probe = path.to_path_buf();
    let mut missing = false;
    loop {
        match fs::metadata(&probe) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Check::new(name, FAIL, "path exists but is not a directory")
                    .details([("path".to_owned(), path.display().to_string())]);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing = true;
                let parent = probe.parent().map(Path::to_path_buf).unwrap_or_default();
                if parent == probe || parent.as_os_str().is_empty() {
                    return Check::new(name, FAIL, "no existing parent directory")
                        .details([("path".to_owned(), path.display().to_string())]);
                }
                probe = parent;
            }
            Err(error) => {
                return Check::new(name, FAIL, format!("cannot inspect path: {error}"))
                    .details([("path".to_owned(), path.display().to_string())]);
            }
        }
    }
    let token = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
    let temporary = probe.join(format!(".symbrowse-doctor-{}-{token}", std::process::id()));
    let file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
    {
        Ok(file) => file,
        Err(error) => {
            return Check::new(name, FAIL, format!("directory is not writable: {error}")).details(
                [
                    ("path".to_owned(), path.display().to_string()),
                    ("probe_dir".to_owned(), probe.display().to_string()),
                ],
            );
        }
    };
    let close_error = file.sync_all().err();
    drop(file);
    let remove_error = fs::remove_file(&temporary).err();
    if close_error.is_some() || remove_error.is_some() {
        return Check::new(name, FAIL, "temporary write probe could not be cleaned up").details([
            ("path".to_owned(), path.display().to_string()),
            ("probe_dir".to_owned(), probe.display().to_string()),
        ]);
    }
    if missing {
        Check::new(
            name,
            WARN,
            "path does not exist yet; nearest existing parent is writable",
        )
        .details([
            ("path".to_owned(), path.display().to_string()),
            ("probe_dir".to_owned(), probe.display().to_string()),
        ])
    } else {
        Check::new(name, PASS, "directory is writable")
            .details([("path".to_owned(), path.display().to_string())])
    }
}

fn socket_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        env::var_os("HOME")
            .map_or_else(env::temp_dir, PathBuf::from)
            .join("Library/Caches/symbrowse/run")
    } else if cfg!(windows) {
        let base = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("USERPROFILE").map_or_else(env::temp_dir, |home| {
                    PathBuf::from(home).join("AppData").join("Local")
                })
            });
        base.join("symbrowse").join("run")
    } else {
        env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("HOME").map_or_else(env::temp_dir, |home| {
                    PathBuf::from(home).join(".cache/symbrowse/run")
                })
            })
            .join("symbrowse")
    }
}

fn check_key_source() -> Check {
    let resolver = KeyResolver::new(SystemKeySources::default());
    match resolver.resolve() {
        Err(error) => Check::new("key_source", WARN, format!("state encryption key source could not be resolved; run `symbrowse state key init` after fixing the provider: {error}"))
            .details([("fix".to_owned(), "symbrowse state key init".to_owned())]),
        Ok(None) => Check::new("key_source", WARN, "no state encryption key configured (key_source: none); run `symbrowse state key init` to enable encryption")
            .details([("source".to_owned(), "none".to_owned()), ("fix".to_owned(), "symbrowse state key init".to_owned())]),
        Ok(Some(key)) => Check::new("key_source", PASS, format!("state encryption key resolved from {}", key.source()))
            .details([("source".to_owned(), key.source().to_owned())]),
    }
}

fn check_guard() -> Check {
    let override_value = env::var("SYMBROWSE_SYMGUARD").unwrap_or_default();
    let override_value = override_value.trim();
    if ["0", "none", "off", "false", "disable"].contains(&override_value.to_lowercase().as_str()) {
        return missing_guard();
    }
    let guard_path = if !override_value.trim().is_empty() {
        Some(PathBuf::from(override_value.trim()))
    } else {
        find_in_path("symbrain")
    };
    if let Some(path) = guard_path {
        let path_text = path.display().to_string();
        let command = if !override_value.is_empty() {
            path_text.clone()
        } else {
            format!("{path_text} guard decide")
        };
        Check::new(
            "guard",
            PASS,
            format!("risk decisions delegated to {command}"),
        )
        .details([
            ("binary".to_owned(), "symbrain".to_owned()),
            ("path".to_owned(), path_text),
            ("command".to_owned(), command),
        ])
    } else {
        missing_guard()
    }
}

fn missing_guard() -> Check {
    Check::new("guard", WARN, "no external risk decider found (symbrain not on PATH and SYMBROWSE_SYMGUARD unset); risk decisions fall back to the built-in policy")
        .details([("override".to_owned(), "SYMBROWSE_SYMGUARD".to_owned())])
}

fn fixes(executable: &str) -> Vec<String> {
    let override_path = if executable.is_empty() {
        "/absolute/path/to/Chrome"
    } else {
        executable
    };
    let install = if cfg!(target_os = "macos") {
        "Install Chrome with: brew install --cask google-chrome (or install Chromium/Edge from its official macOS installer)."
    } else if cfg!(target_os = "linux") {
        "Install Chromium with: sudo apt install chromium (or use your distribution's Chromium/Chrome/Edge package)."
    } else if cfg!(windows) {
        "Install Chrome, Chromium, or Edge with winget, for example: winget install Google.Chrome."
    } else {
        "Install Google Chrome, Chromium, or Microsoft Edge using your platform's package manager or official installer."
    };
    vec![
        "No changes were made; --fix only prints guidance.".to_owned(),
        install.to_owned(),
        format!(
            "Set an explicit browser when it is installed elsewhere: export SYMBROWSE_EXECUTABLE_PATH={}",
            shell_quote(override_path)
        ),
        "Rerun the checks with: symbrowse doctor (or symbrowse doctor --json).".to_owned(),
    ]
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
}

#[cfg(test)]
mod tests {
    use super::check_engine;

    #[test]
    fn doctor_reports_default_launch_mode_and_real_script_disabler() {
        let check = check_engine();
        let details = check.details.expect("engine details");
        let capabilities = symbrowse_engine_chrome::canonical_capabilities();

        assert_eq!(
            details.get("launch_mode").map(String::as_str),
            Some("launch")
        );
        assert_eq!(
            details.get("interfaces").map(String::as_str),
            Some(capabilities.interfaces.join(",").as_str())
        );
        assert!(
            capabilities
                .interfaces
                .iter()
                .any(|name| name == "ScriptDisabler")
        );
        assert!(
            !capabilities
                .unsupported
                .iter()
                .any(|name| name == "ScriptDisabler")
        );
        assert_eq!(
            details.get("unsupported").map(String::as_str),
            Some(capabilities.unsupported.join(",").as_str())
        );
    }
}
