#![allow(clippy::items_after_statements, clippy::manual_let_else)]
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct OracleFile {
    path: String,
    #[serde(rename = "type")]
    file_type: String,
    mode: u32,
    #[serde(default)]
    bytes: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OracleCase {
    id: String,
    args: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
    files: Vec<OracleFile>,
}

#[derive(Debug, Deserialize)]
struct Oracle {
    schema_version: u32,
    go_revision: String,
    generator_sha256: String,
    go_sources: BTreeMap<String, String>,
    cases: Vec<OracleCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedFile {
    path: String,
    file_type: String,
    mode: u32,
    bytes: Option<Vec<u8>>,
    target: Option<String>,
}

fn fixture() -> Oracle {
    let name = if cfg!(target_os = "macos") {
        "install_oracle_darwin.json"
    } else if cfg!(target_os = "linux") {
        "install_oracle_linux.json"
    } else {
        panic!("install oracle has no fixture for this target OS")
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../symbrain-harness/tests/fixtures")
        .join(name);
    serde_json::from_slice(&fs::read(path).expect("install oracle fixture exists"))
        .expect("install oracle fixture is valid")
}

fn decode_base64(value: &str) -> Vec<u8> {
    // The generator uses standard RFC 4648 base64. Keeping this decoder local
    // avoids making the production crates depend on a fixture-only crate.
    let mut output = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    for byte in value.bytes() {
        if byte == b'=' {
            break;
        }
        let digit = u32::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 byte {byte:?}"),
        });
        buffer = (buffer << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from(buffer >> bits).unwrap());
            buffer &= (1 << bits) - 1;
        }
    }
    output
}

fn setup(case_id: &str, root: &Path) {
    let home = root.join("home");
    let config = root.join("config");
    let project = root.join("project");
    match case_id {
        "default_profile" | "default_profile_env_override" => write(
            &config.join("symbrain/config.toml"),
            b"default_profile = \"personal\"\n",
            0o600,
        ),
        "malformed_global_config" => write(
            &config.join("symbrain/config.toml"),
            b"invalid = [ unterminated toml\n",
            0o600,
        ),
        "malformed_refuses_before_backup" => {
            write(&home.join(".claude.json"), b"{not valid json", 0o600);
        }
        "superseded_basenames_keep_vault" | "keep_superseded" => write(
            &home.join(".cursor/mcp.json"),
            br#"{"mcpServers":{"old-memory":{"command":"/opt/bin/symmemory"},"old-skills":{"command":"symskills"},"vault":{"command":"symvault"},"other":{"command":"other"}}}"#,
            0o640,
        ),
        "dry_run" | "absent_entry_uninstall_noop" => write(
            &home.join(".cursor/mcp.json"),
            br#"{"mcpServers":{"other":{"command":"other"}}}"#,
            0o640,
        ),
        "foreign_named_symbrain_uninstall_noop" => write(
            &home.join(".cursor/mcp.json"),
            br#"{"mcpServers":{"symbrain":{"command":"not-symbrain","args":["x"]}}}"#,
            0o640,
        ),
        "uninstall_removes_symbrain" => write(
            &home.join(".cursor/mcp.json"),
            br#"{"mcpServers":{"other":{"command":"other"},"symbrain":{"command":"symbrain","args":["mcp","--profile","personal"]}}}"#,
            0o640,
        ),
        "backup_bytes_mode_timestamp_shape" => {
            let path = home.join(".cursor/mcp.json");
            write(&path, br#"{"mcpServers":{"other":{"command":"other"}}}"#, 0o640);
            write(&path.with_extension("json.bak.20260909T000000Z"), b"reserved rollback", 0o600);
        }
        _ => {}
    }
    let _ = project;
}

fn write(path: &Path, bytes: &[u8], mode: u32) {
    let parent = path.parent().expect("fixture parent");
    fs::create_dir_all(parent).unwrap();
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn normalize(value: &[u8], root: &Path) -> String {
    let mut text = String::from_utf8(value.to_vec())
        .unwrap()
        .replace(root.to_string_lossy().as_ref(), "<root>");
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(".bak.") {
        let start = cursor + relative;
        let end = text[start..]
            .find([')', '\n'])
            .map_or(text.len(), |offset| start + offset);
        if text[start..end].starts_with(".bak.") {
            text.replace_range(start..end, ".bak.<timestamp>");
            cursor = start + ".bak.<timestamp>".len();
        } else {
            cursor = end;
        }
    }
    text
}

fn snapshot(root: &Path) -> Vec<ObservedFile> {
    let mut files = Vec::new();
    fn walk(root: &Path, path: &Path, out: &mut Vec<ObservedFile>) {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let relative = relative
                .split_once(".bak.")
                .map_or(relative.clone(), |(prefix, _)| {
                    format!("{prefix}.bak.<timestamp>")
                });
            let metadata = fs::symlink_metadata(&path).unwrap();
            let mode = permissions_mode(&metadata);
            if metadata.file_type().is_symlink() {
                out.push(ObservedFile {
                    path: relative,
                    file_type: "symlink".into(),
                    mode,
                    bytes: None,
                    target: Some(normalize(
                        fs::read_link(&path).unwrap().to_string_lossy().as_bytes(),
                        root,
                    )),
                });
            } else if metadata.is_dir() {
                out.push(ObservedFile {
                    path: relative.clone(),
                    file_type: "dir".into(),
                    mode,
                    bytes: None,
                    target: None,
                });
                walk(root, &path, out);
            } else if metadata.is_file() {
                out.push(ObservedFile {
                    path: relative,
                    file_type: "file".into(),
                    mode,
                    bytes: Some(fs::read(&path).unwrap()),
                    target: None,
                });
            }
        }
    }
    walk(root, root, &mut files);
    files.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.mode.cmp(&b.mode))
            .then(a.bytes.cmp(&b.bytes))
    });
    files
}

fn permissions_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o777
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn expected_files(files: &[OracleFile]) -> Vec<ObservedFile> {
    files
        .iter()
        .map(|file| ObservedFile {
            path: file.path.clone(),
            file_type: file.file_type.clone(),
            mode: file.mode,
            bytes: file.bytes.as_deref().map(decode_base64),
            target: file.target.clone(),
        })
        .collect()
}

fn run_case(case: &OracleCase) -> (i32, String, String, Vec<ObservedFile>) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for dir in ["home", "config", "data", "cache", "project"] {
        let path = root.join(dir);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    setup(&case.id, &root);
    let args: Vec<_> = case
        .args
        .iter()
        .map(|arg| {
            if arg == "PROJECT" {
                root.join("project").to_string_lossy().into_owned()
            } else {
                arg.clone()
            }
        })
        .collect();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .args(args)
        .current_dir(root.join("project"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC");
    if case.id == "default_profile_env_override" {
        command.env("SYMBRAIN_DEFAULT_PROFILE", "restricted");
    }
    let output = command.output().unwrap();
    (
        output.status.code().unwrap_or(1),
        normalize(&output.stdout, &root),
        normalize(&output.stderr, &root),
        snapshot(&root),
    )
}

#[test]
fn install_oracle_fixture_is_consumed_by_the_native_rust_cli() {
    let oracle = fixture();
    assert_eq!(oracle.schema_version, 1);
    assert!(!oracle.go_revision.is_empty());
    assert!(!oracle.generator_sha256.is_empty());
    assert_eq!(oracle.go_sources.len(), 13);
    for case in &oracle.cases {
        let (exit, stdout, stderr, files) = run_case(case);
        assert_eq!(exit, case.exit, "{} exit", case.id);
        assert_eq!(stdout, case.stdout, "{} stdout", case.id);
        assert_eq!(stderr, case.stderr, "{} stderr", case.id);
        assert_eq!(files, expected_files(&case.files), "{} filesystem", case.id);
    }
}

#[test]
fn install_oracle_comparison_rejects_intentional_mutation() {
    let oracle = fixture();
    let case = oracle
        .cases
        .iter()
        .find(|case| case.id == "fresh_entry_cursor" || case.id == "fresh_entry_bytes_and_modes")
        .unwrap();
    let (_, _, _, observed) = run_case(case);
    let mut mutated = expected_files(&case.files);
    mutated[0].mode ^= 1;
    assert_ne!(observed, mutated, "comparison must detect a mode mutation");
}
