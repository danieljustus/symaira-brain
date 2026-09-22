use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::SystemTime};

fn list(binary: &Path, root: &Path) -> Value {
    let home = root.join("home");
    let mut command = Command::new(binary);
    command
        .args(["flow", "list", "--json"])
        .current_dir(root)
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("LOCALAPPDATA", home.join("local"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("PATH", root.join("empty-path"));
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let output = command.output().expect("run isolated flow list");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("flow list JSON")
}

#[test]
fn flow_list_preserves_go_json_envelope_for_empty_and_populated_discovery() {
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "symbrowse-flow-list-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let rust = Path::new(env!("CARGO_BIN_EXE_symbrowse"));
    let go = std::env::var_os("SYMBROWSE_GO_ORACLE");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../testdata/port/workflows/workflows.json"
    ))
    .unwrap();
    let flow_path = Path::new(".symbrowse").join("flows").join("capture.yml");
    let contents = serde_json::to_vec(&fixture["flow"]).unwrap();

    // Observed from Go newFlowListCommand at a92385d2deecc08d1fd96869908b81b7abd355fe
    // (Brain #662). An empty Go discovery is [], not null. CI also compares the
    // same commands against a real Go binary, without rewriting either result.
    for populated in [false, true] {
        let root = root.join(if populated { "populated" } else { "empty" });
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("home")).unwrap();
        fs::create_dir(root.join("empty-path")).unwrap();
        let entries = if populated {
            fs::create_dir_all(root.join(flow_path.parent().unwrap())).unwrap();
            fs::write(root.join(&flow_path), &contents).unwrap();
            json!([{
                "name": "fixture-flow", "path": flow_path, "origin": "project",
                "version": 1, "steps": 5
            }])
        } else {
            json!([])
        };
        let expected = json!({"success": true, "data": {"flows": entries}});
        let actual = list(rust, &root);
        assert_eq!(actual, expected, "populated={populated}");
        if let Some(binary) = &go {
            assert_eq!(actual, list(Path::new(binary), &root), "Go/Rust parity");
        }
        assert_eq!(fs::read_dir(root.join("home")).unwrap().count(), 0);
        assert_eq!(
            fs::read_dir(&root).unwrap().count(),
            if populated { 3 } else { 2 }
        );
        if populated {
            assert_eq!(fs::read(root.join(&flow_path)).unwrap(), contents);
            assert_eq!(
                fs::read_dir(root.join(".symbrowse/flows")).unwrap().count(),
                1
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
