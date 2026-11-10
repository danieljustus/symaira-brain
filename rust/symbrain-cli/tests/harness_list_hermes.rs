//! #1024: `harness list` shows Hermes (read-only) from `$HERMES_HOME` and its
//! profile configs; a decoy under `$HOME/.hermes` must not leak in.

#![deny(unsafe_code)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::process::Command;

use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let root = root.path().canonicalize().unwrap();
    let home = root.join("home");
    for path in [&home, &root.join("project")] {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .current_dir(root.join("project"))
        .args(args);
    command
}

#[test]
fn hermes_honours_hermes_home_and_lists_profile_configs() {
    let root = TempDir::new().unwrap();
    let hermes_home = root.path().canonicalize().unwrap().join("alt-hermes");
    std::fs::create_dir_all(hermes_home.join("profiles/work")).unwrap();
    std::fs::create_dir_all(hermes_home.join("profiles/broken")).unwrap();
    std::fs::create_dir_all(hermes_home.join("profiles/empty")).unwrap();
    std::fs::write(
        hermes_home.join("config.yaml"),
        "mcp_servers:\n  github:\n    command: npx\n    env:\n      TOKEN: secret-value\n",
    )
    .unwrap();
    std::fs::write(
        hermes_home.join("profiles/work/config.yaml"),
        "mcp_servers:\n  remote:\n    url: https://example.test/mcp\n",
    )
    .unwrap();
    std::fs::write(
        hermes_home.join("profiles/broken/config.yaml"),
        "mcp_servers: [\n",
    )
    .unwrap();
    // A decoy under the real HOME must be ignored while HERMES_HOME is set.
    write_hermes_decoy(&root);

    let table = command(&root, &["harness", "list"])
        .env("HERMES_HOME", &hermes_home)
        .output()
        .unwrap();
    assert!(table.status.success() && table.stderr.is_empty());
    let text = String::from_utf8(table.stdout).unwrap();
    let block = &text[text.find("hermes\tHermes\n").unwrap()..];
    assert!(block.contains(&format!(
        "  global\t{}\tparsed\tservers=github[stdio]\n",
        hermes_home.join("config.yaml").display()
    )));
    assert!(block.contains("  profile work\t"));
    assert!(block.contains("\tparsed\tservers=remote[http]\n"));
    assert!(block.contains("  profile broken\t"));
    assert!(block.contains(
        "\tinvalid\tservers=(none)\n    error: harness: hermes config is not valid yaml"
    ));
    assert!(
        !block.contains("profile empty"),
        "profiles without config.yaml are not listed"
    );
    assert!(!block.contains("decoy"));
    assert!(!text.contains("secret-value"));

    let json = command(&root, &["harness", "list", "--json"])
        .env("HERMES_HOME", &hermes_home)
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let hermes = value["harnesses"].as_array().unwrap().last().unwrap();
    assert_eq!(hermes["name"], "hermes");
    assert_eq!(hermes["read_only"], true);
    assert_eq!(hermes["global"]["servers"][0]["env_names"][0], "TOKEN");
    let names: Vec<_> = hermes["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["broken", "work"]);
}

fn write_hermes_decoy(root: &TempDir) {
    let dir = root.path().join("home/.hermes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.yaml"),
        "mcp_servers:\n  decoy:\n    command: x\n",
    )
    .unwrap();
}
