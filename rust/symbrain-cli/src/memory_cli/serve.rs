//! `symbrain memory serve`: the embedded memory HTTP API as a sync peer.
//!
//! Mirrors `cmd/symbrain/cmd_memory_serve.go`: the Go `flag` grammar of
//! `--port`/`--db`, the usage text, the stdout banner printed before the bind
//! attempt, the runtime assembly of `buildMemoryHTTPServer` (config, database,
//! JWT signing secret) and the bounded loopback server of
//! `symbrain_memory::http`.

use super::{OsString, config, flag_integer, flags, paths};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_memory::http::{Options, Server};
use symbrain_memory::{EmbeddingGenerator, Store};

/// Not the Go-era 8787, which Readarr (the *arr stack) owns; 48787 sits below
/// the OS ephemeral range and has no well-known self-hosted user (#996).
const DEFAULT_PORT: i64 = 48787;

const SERVE_USAGE: &str = "symbrain memory serve — run the memory HTTP API as a sync peer

Usage:
  symbrain memory serve [flags]

Flags:
  --port <n>   TCP port to listen on (default 48787). Always bound to
               127.0.0.1; never exposed on a non-loopback address. Startup
               is refused when another process already serves the same port
               on [::1] or the wildcard address.
  --db <path>  Database path override (default: the standard memory
               database under the XDG data directory).

This starts the same HTTP API 'symbrain memory sync --remote' talks to:
/api/sync/changes, /api/sync/apply, /api/sync/relay, plus /api/search,
/api/set, /api/list, /api/get, /api/delete, /api/rules, /api/stats and
/api/status. All routes except /api/status require a bearer token; see
'symbrain memory sync --help' for how tokens are supplied to the client
side of a sync. The process runs until interrupted (Ctrl-C/SIGTERM), at
which point it shuts down gracefully.
";

/// Whether `args` (after the `serve` verb) can be handled natively. Every
/// invocation is: the flag errors, help and the server itself are all ported.
pub(super) fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let (port, db) = match parse(args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let store = match paths::open_store(stderr, "serve", db.as_deref()) {
        Ok(store) => store,
        Err(code) => return code,
    };
    let server = match build(store) {
        Ok(server) => server,
        Err(message) => {
            let _ = writeln!(stderr, "symbrain memory serve: {message}");
            return exit::GENERIC;
        }
    };
    let _ = writeln!(
        stdout,
        "symbrain memory serve: listening on http://127.0.0.1:{port}"
    );
    let _ = stdout.flush();
    let port = u16::try_from(port).unwrap_or(0);
    match server.run_loopback(port) {
        Ok(()) => exit::OK,
        Err(message) => {
            let _ = writeln!(stderr, "symbrain memory serve: {message}");
            exit::GENERIC
        }
    }
}

fn os_from(value: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(value.to_vec())
}

fn usage(stderr: &mut dyn Write) {
    let _ = write!(stderr, "{SERVE_USAGE}");
}

/// The Go `flag.FlagSet` for `--port`/`--db` plus the post-parse checks.
fn parse(args: &[OsString], stderr: &mut dyn Write) -> Result<(i64, Option<OsString>), u8> {
    let normalized = crate::normalize_flags(args);
    let mut port = DEFAULT_PORT;
    let mut db: Option<Vec<u8>> = None;
    let mut index = 0;
    while let Some(argument) = normalized.get(index) {
        let argument = flags::bytes(argument);
        if argument == b"--" {
            index += 1;
            break;
        }
        if !argument.starts_with(b"-") || argument == b"-" {
            break;
        }
        let flag = argument.strip_prefix(b"--").unwrap_or(&argument[1..]);
        let (name, inline) = flag
            .iter()
            .position(|&b| b == b'=')
            .map_or((flag, None), |n| (&flag[..n], Some(&flag[n + 1..])));
        if name.is_empty() || name.starts_with(b"-") || name.starts_with(b"=") {
            let _ = stderr.write_all(b"bad flag syntax: ");
            let _ = stderr.write_all(&argument);
            let _ = stderr.write_all(b"\n");
            usage(stderr);
            return Err(exit::USAGE);
        }
        let is_port = match name {
            b"port" => true,
            b"db" => false,
            _ => {
                if !matches!(name, b"h" | b"help") {
                    let _ = stderr.write_all(b"flag provided but not defined: -");
                    let _ = stderr.write_all(name);
                    let _ = stderr.write_all(b"\n");
                }
                usage(stderr);
                return Err(exit::USAGE);
            }
        };
        let value = inline.map(<[u8]>::to_vec).or_else(|| {
            index += 1;
            normalized.get(index).map(|arg| flags::bytes(arg))
        });
        let Some(value) = value else {
            let _ = stderr.write_all(b"flag needs an argument: -");
            let _ = stderr.write_all(name);
            let _ = stderr.write_all(b"\n");
            usage(stderr);
            return Err(exit::USAGE);
        };
        if is_port {
            match flag_integer::parse_integer(&value) {
                Ok(number) => port = number,
                Err(reason) => {
                    let _ = writeln!(
                        stderr,
                        "invalid value {} for flag -port: {reason}",
                        format_go_quoted(&os_from(&value))
                    );
                    usage(stderr);
                    return Err(exit::USAGE);
                }
            }
        } else {
            db = Some(value);
        }
        index += 1;
    }
    if let Some(extra) = normalized.get(index) {
        let _ = writeln!(
            stderr,
            "symbrain memory serve: unexpected argument {}",
            format_go_quoted(extra)
        );
        return Err(exit::USAGE);
    }
    if port <= 0 || port > 65535 {
        let _ = writeln!(
            stderr,
            "symbrain memory serve: --port must be between 1 and 65535"
        );
        return Err(exit::USAGE);
    }
    Ok((port, db.map(|value| os_from(&value))))
}

/// `buildMemoryHTTPServer` after the database is open: JWT provider and the
/// MCP/HTTP server around the shared store.
fn build(store: Store) -> Result<Arc<Server>, String> {
    let cfg = config::load();
    let secret = jwt_secret(cfg).map_err(|error| format!("init memory JWT provider: {error}"))?;
    let options = Options {
        version: option_env!("SYMBRAIN_VERSION").unwrap_or("dev").to_owned(),
        require_profile: cfg.require_profile,
        audit_enabled: cfg.audit_log_enabled,
        trusted_proxies: cfg.trusted_proxies.clone(),
        quantize_binary: cfg.quantize_binary,
        prefilter: cfg.prefilter,
        conflict: super::write::conflict_config(cfg),
        working_ttl: go_duration(&cfg.working_ttl)
            .filter(|ttl| !ttl.is_zero())
            .unwrap_or(Duration::from_hours(24)),
    };
    let generator = EmbeddingGenerator::new(&cfg.ollama_url, &cfg.ollama_model);
    Server::new(Arc::new(store), secret, generator, options)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

/// `time.ParseDuration` for the `working_memory.ttl` shapes in use
/// (`24h`, `90m`, `1h30m`, `1.5h`, `500ms`); anything else is `None`, which
/// falls back to the shipped 24h default.
fn go_duration(text: &str) -> Option<Duration> {
    let mut rest = text.strip_prefix('+').unwrap_or(text);
    if rest.is_empty() || rest.starts_with('-') {
        return None;
    }
    if rest == "0" {
        return Some(Duration::ZERO);
    }
    let mut total = 0.0_f64;
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let number: f64 = rest[..end].parse().ok()?;
        rest = &rest[end..];
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let scale = match &rest[..unit_end] {
            "ns" => 1e-9,
            "us" | "µs" | "μs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            _ => return None,
        };
        total += number * scale;
        rest = &rest[unit_end..];
    }
    Duration::try_from_secs_f64(total).ok()
}

/// Go's wording of an `os` path error reason.
fn go_reason(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(2) => "no such file or directory".to_owned(),
        Some(13) => "permission denied".to_owned(),
        Some(17) => "file exists".to_owned(),
        Some(20) => "not a directory".to_owned(),
        Some(21) => "is a directory".to_owned(),
        Some(30) => "read-only file system".to_owned(),
        _ => {
            let text = error.to_string();
            let base = text.split(" (os error").next().unwrap_or(&text);
            let mut chars = base.chars();
            chars
                .next()
                .map(|first| first.to_lowercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}

/// `security.NewJWTProvider`'s secret resolution: the configured secret
/// reference, `JWT_SECRET_KEY`, the persisted file, then a fresh 32-byte hex
/// secret persisted with mode 0600 in a 0700 directory.
fn jwt_secret(cfg: &config::Config) -> Result<Vec<u8>, String> {
    let mut secret = symbrain_usage::resolve_reference(&cfg.jwt_secret, "JWT_SECRET_KEY")
        .map_err(|error| format!("JWT secret symvault:// resolution failed: {error}"))?;
    if secret.is_empty() {
        secret = std::env::var_os("JWT_SECRET_KEY")
            .map(|value| flags::go_string(&flags::bytes(&value)))
            .unwrap_or_default();
    }
    let path = secret_path(cfg);
    if secret.is_empty()
        && let Some(path) = &path
        && let Ok(data) = std::fs::read(path)
    {
        flags::go_string(&data).trim().clone_into(&mut secret);
    }
    if secret.is_empty() {
        let Some(path) = path else {
            return Err("failed to generate JWT secret: cannot resolve the secret path".to_owned());
        };
        let mut bytes = [0_u8; 32];
        getrandom_fill(&mut bytes)?;
        secret = bytes
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0x0f])
            .map(|nibble| char::from_digit(u32::from(nibble), 16).unwrap_or('0'))
            .collect();
        persist(&path, &secret)
            .map_err(|error| format!("failed to generate JWT secret: {error}"))?;
    }
    Ok(secret.into_bytes())
}

fn getrandom_fill(bytes: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(bytes))
        .map_err(|error| format!("failed to generate JWT secret: {}", go_reason(&error)))
}

fn persist(path: &std::path::Path, secret: &str) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|error| format!("mkdir {}: {}", dir.display(), go_reason(&error)))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("open {}: {}", path.display(), go_reason(&error)))?;
    file.write_all(format!("{secret}\n").as_bytes())
        .map_err(|error| format!("write {}: {}", path.display(), go_reason(&error)))
}

/// `cfg.JWT.SecretPath`, else `<memory config dir>/jwt.secret`.
fn secret_path(cfg: &config::Config) -> Option<PathBuf> {
    if !cfg.jwt_secret_path.is_empty() {
        return Some(PathBuf::from(&cfg.jwt_secret_path));
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))?;
    let current = base.join("symbrain/memory");
    let legacy = base.join("symmemory");
    let dir = if current.is_dir() || !legacy.is_dir() {
        current
    } else {
        legacy
    };
    Some(dir.join("jwt.secret"))
}
