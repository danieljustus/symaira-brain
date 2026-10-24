//! Native store/directory creation matches the immutable Go process (#769).
//!
//! Expected bytes and modes are the Go oracle (`dcddcef0`) observations from
//! `scripts/memory-cli-oracle/creation_cases.py`, identical on macOS/APFS and
//! Linux/ext4-overlay. The live Go-vs-native comparison runs in that harness;
//! this process test keeps the contract enforced by plain `cargo test`.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(root: &Path, args: &[&str]) -> Run {
    // Fixed umask: modes are `0777 & !umask` for creation requests.
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg("umask 022; exec \"$0\" \"$@\"")
        .arg(env!("CARGO_BIN_EXE_symbrain"))
        .arg("memory")
        .args(args)
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "")
        .env("SYMBRAIN_GO_BINARY", root.join("absent-go"))
        .current_dir(root)
        .output()
        .expect("actual CLI child");
    Run {
        code: output.status.code().expect("exit status"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).replace(root.to_str().unwrap(), "<root>"),
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("entry").permissions().mode() & 0o7777
}

/// The canonical root: Go reports the resolved path of trusted system links.
fn temp() -> (tempfile::TempDir, std::path::PathBuf) {
    let guard = tempfile::tempdir().expect("root");
    let root = guard.path().canonicalize().expect("canonical root");
    (guard, root)
}

const SET: [&str; 5] = ["set", "hello", "--kind", "fact", "--staged"];

#[test]
fn new_store_gets_private_directories_and_database() {
    let (_guard, root) = temp();
    let database = root.join("a/b/c.db");
    let path = database.to_str().unwrap();
    let result = run(&root, &[&SET[..], &["--db", path, "--json"]].concat());
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        result.stdout.contains("\"staged\":true"),
        "{}",
        result.stdout
    );
    assert_eq!(mode(&root.join("a")), 0o700);
    assert_eq!(mode(&root.join("a/b")), 0o700);
    assert_eq!(mode(&database), 0o600);
}

#[test]
fn delete_on_missing_store_creates_it_then_reports_not_found() {
    let (_guard, root) = temp();
    let database = root.join("n/x.db");
    let result = run(
        &root,
        &["delete", "nope", "--db", database.to_str().unwrap()],
    );
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "symbrain memory delete: memory not found: nope\n"
    );
    assert_eq!(mode(&root.join("n")), 0o700);
    assert_eq!(mode(&database), 0o600);
}

#[test]
fn existing_directories_keep_their_modes_and_new_children_are_private() {
    let (_guard, root) = temp();
    let wide = root.join("wide");
    std::fs::create_dir(&wide).expect("wide");
    std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o777)).expect("mode");
    let database = wide.join("sub/x.db");
    let result = run(&root, &["list", "--db", database.to_str().unwrap()]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(mode(&wide), 0o777);
    assert_eq!(mode(&wide.join("sub")), 0o700);
}

#[test]
fn directory_creation_failures_use_the_shipped_diagnostics() {
    let (_guard, root) = temp();
    std::fs::write(root.join("file"), "x").expect("file");
    std::fs::create_dir(root.join("dbdir")).expect("dir");
    std::os::unix::fs::symlink(root.join("nowhere"), root.join("link")).expect("symlink");
    let wrap = "failed to open sqlite database: ";
    let cases = [
        (
            "file/x.db",
            format!(
                "{wrap}failed to create database directory: mkdir <root>/file: not a directory"
            ),
        ),
        (
            "link/x.db",
            format!(
                "{wrap}failed to create database directory: mkdir <root>/link: too many levels of symbolic links"
            ),
        ),
        (
            "dbdir",
            format!("{wrap}failed to open sqlite database: unable to open database file (14)"),
        ),
    ];
    for verb in ["list", "rules", "query-log", "set", "delete"] {
        for (relative, detail) in &cases {
            let database = root.join(relative);
            let mut args = vec![verb];
            if verb == "set" {
                args.extend(["hello", "--kind", "fact", "--staged"]);
            } else if verb == "delete" {
                args.push("a");
            }
            args.extend(["--db", database.to_str().unwrap()]);
            let result = run(&root, &args);
            assert_eq!(result.code, 1, "{verb} {relative}");
            assert_eq!(result.stdout, "");
            assert_eq!(
                result.stderr,
                format!("symbrain memory {verb}: open memory database: {detail}\n"),
                "{verb} {relative}"
            );
        }
    }
}

#[test]
fn read_only_parent_reports_directory_and_file_errors() {
    let (_guard, root) = temp();
    let ro = root.join("ro");
    std::fs::create_dir(&ro).expect("ro");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).expect("mode");
    if std::fs::create_dir(ro.join("probe")).is_ok() {
        return; // privileged runner: permission denial is not observable
    }
    let wrap = "failed to open sqlite database: ";
    let sub = run(
        &root,
        &["list", "--db", ro.join("sub/x.db").to_str().unwrap()],
    );
    assert_eq!(sub.code, 1);
    assert_eq!(
        sub.stderr,
        format!(
            "symbrain memory list: open memory database: {wrap}failed to create database directory: mkdir <root>/ro/sub: permission denied\n"
        )
    );
    let file = run(
        &root,
        &["delete", "a", "--db", ro.join("x.db").to_str().unwrap()],
    );
    assert_eq!(file.code, 1);
    assert_eq!(
        file.stderr,
        format!(
            "symbrain memory delete: open memory database: {wrap}{wrap}unable to open database file (14)\n"
        )
    );
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o700)).expect("cleanup");
}

/// Splits `<prefix><description> (<code>)\n` into its description and code.
fn description_and_code(stderr: &str) -> Option<(&str, u32)> {
    let body = stderr.strip_suffix(")\n")?;
    let (description, code) = body.rsplit_once(" (")?;
    Some((description, code.parse().ok()?))
}

#[test]
fn non_regular_database_paths_report_the_shipped_doubled_open_error() {
    // Go opens FIFOs and `/dev/null` without blocking and fails while
    // connecting; no verb may fall back to Go (absent here) or print a bare
    // SQLite message. The SQLite code is environment-dependent (`/dev/null`:
    // `unable to open database file (14)` on macOS, `attempt to write a
    // readonly database (1544)` on Linux as a non-root user), so this test
    // pins only the shape; the live Go replay (`store_creation` family of
    // `scripts/memory-cli-oracle/replay.py`) compares the exact bytes on
    // every runner.
    let (_guard, root) = temp();
    let fifo = root.join("fifo");
    let status = Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");
    assert!(status.success());
    let wrap = "failed to open sqlite database: failed to open sqlite database: ";
    for database in ["/dev/null", fifo.to_str().unwrap()] {
        for verb in ["list", "rules", "query-log", "set", "delete", "search"] {
            let mut args = vec![verb];
            match verb {
                "set" => args.extend(["hello", "--kind", "fact", "--staged"]),
                "delete" => args.push("a"),
                "search" => args.push("x"),
                _ => {}
            }
            args.extend(["--db", database]);
            let result = run(&root, &args);
            let prefix = format!("symbrain memory {verb}: open memory database: {wrap}");
            assert_eq!(result.code, 1, "{verb} {database}: {}", result.stderr);
            assert_eq!(result.stdout, "", "{verb} {database}");
            let detail = result
                .stderr
                .strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("{verb} {database}: {}", result.stderr));
            let (description, code) = description_and_code(detail)
                .unwrap_or_else(|| panic!("{verb} {database}: not `<text> (<code>)`: {detail:?}"));
            assert!(
                !description.is_empty() && !description.contains([':', '<']) && code > 0,
                "{verb} {database}: {detail:?}"
            );
        }
    }
}

#[test]
fn description_and_code_parses_only_the_shipped_shape() {
    assert_eq!(
        description_and_code("disk I/O error (10)\n"),
        Some(("disk I/O error", 10))
    );
    assert_eq!(
        description_and_code("attempt to write a readonly database (1544)\n"),
        Some(("attempt to write a readonly database", 1544))
    );
    for bad in ["", "no code\n", "text (x)\n", "text (1)", "(1)\n"] {
        assert_eq!(description_and_code(bad), None, "{bad:?}");
    }
}

#[test]
fn relative_database_path_honours_only_pwd_naming_the_current_directory() {
    use std::os::unix::fs::MetadataExt;

    // Observed against Go dcddcef0: valid logical PWD preserves the symlink
    // rejection; absent, relative or unrelated PWD uses the physical cwd.
    for args in [&SET[..], &["delete", "absent"][..]] {
        for spelling in [
            "symlink",
            "physical",
            "wrong-directory",
            "relative",
            "unset",
        ] {
            let (_guard, root) = temp();
            let real = root.join("real");
            let link = root.join("link");
            std::fs::create_dir(&real).unwrap();
            std::os::unix::fs::symlink(&real, &link).unwrap();
            if spelling == "symlink" && std::fs::symlink_metadata(&link).unwrap().uid() == 0 {
                eprintln!("SKIP logical-PWD symlink rejection: root-owned links are trusted");
                continue;
            }
            let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
            command
                .arg("memory")
                .args(args)
                .args(["--db", "rel/sub/c.db"])
                .env_clear()
                .env("HOME", &root)
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("XDG_DATA_HOME", root.join("data"))
                .env("XDG_CACHE_HOME", root.join("cache"))
                .env("PATH", "")
                .env("SYMBRAIN_GO_BINARY", root.join("absent-go"))
                .current_dir(&real);
            match spelling {
                "symlink" => command.env("PWD", &link),
                "physical" => command.env("PWD", &real),
                "wrong-directory" => command.env("PWD", &root),
                "relative" => command.env("PWD", "link"),
                _ => &mut command,
            };
            let output = command.output().expect("native CLI child");
            let stderr = String::from_utf8(output.stderr).unwrap();
            let verb = args[0];
            if spelling == "symlink" {
                assert_eq!(output.status.code(), Some(1), "{verb}: {stderr}");
                assert!(output.stdout.is_empty());
                assert_eq!(
                    stderr,
                    format!(
                        "symbrain memory {verb}: open memory database: failed to open sqlite database: failed to create database directory: mkdir {}: too many levels of symbolic links\n",
                        link.display()
                    )
                );
                assert!(
                    !real.join("rel").exists(),
                    "rejection must not create directories"
                );
            } else {
                assert!(
                    real.join("rel/sub/c.db").is_file(),
                    "{verb} {spelling}: {stderr}"
                );
                if verb == "set" {
                    assert!(output.status.success(), "{spelling}: {stderr}");
                    assert!(stderr.is_empty());
                } else {
                    assert_eq!(output.status.code(), Some(1));
                    assert_eq!(stderr, "symbrain memory delete: memory not found: absent\n");
                }
            }
        }
    }
}
