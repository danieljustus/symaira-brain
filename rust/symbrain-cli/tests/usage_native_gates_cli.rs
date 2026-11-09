//! The former credential-gated Go fallback of `symbrain usage` (#768) is gone:
//! every scenario below used to route the whole report to Go and is now answered
//! natively, byte for byte as the recorded Go binary answered it
//! (`scripts/usage-native-gates-oracle/cli.py --record`). The fallback binary
//! is a sentinel that fails loudly, and every scenario is network-free.
#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const ORACLE: &str = include_str!("fixtures/usage_native_gates_cli.json");

fn decode(value: &serde_json::Value) -> Vec<u8> {
    let (mut out, mut buffer, mut bits) = (Vec::new(), 0_u32, 0_u32);
    for byte in value.as_str().expect("base64").bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buffer = (buffer << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(buffer >> bits).expect("byte"));
            buffer &= (1 << bits) - 1;
        }
    }
    out
}

#[test]
fn former_fallback_gates_match_recorded_go_bytes_without_a_go_binary() {
    let oracle: serde_json::Value = serde_json::from_str(ORACLE).expect("oracle");
    let cases = oracle["cases"].as_array().expect("cases");
    assert!(cases.len() >= 60, "oracle lost cases: {}", cases.len());
    let sentinel_dir = tempfile::tempdir().unwrap();
    let sentinel = sentinel_dir.path().join("go-fallback");
    fs::write(
        &sentinel,
        "#!/bin/sh\necho 'unexpected Go fallback' >&2\nexit 97\n",
    )
    .unwrap();
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o700)).unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        for entry in case["files"].as_array().unwrap() {
            let path = home.join(entry["path"].as_str().unwrap());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, entry["contents"].as_str().unwrap()).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args([
                "usage",
                if case["output"] == "json" {
                    "--json"
                } else {
                    "--output=table"
                },
                "--providers",
                &case["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(","),
            ])
            .current_dir(&home)
            .env_clear()
            .envs(coverage::profile_environment())
            .env("PATH", "/nonexistent-768")
            .env("SYMBRAIN_GO_BINARY", &sentinel);
        match case["home"].as_str().unwrap() {
            "set" => {
                command.env("HOME", &home);
            }
            "empty" => {
                command.env("HOME", "");
            }
            _ => {}
        }
        for (name, value) in case["env"].as_object().unwrap() {
            command.env(
                name,
                value
                    .as_str()
                    .unwrap()
                    .replace("$HOME", home.to_str().unwrap()),
            );
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::try_from(case["go"]["exit"].as_i64().unwrap()).unwrap()),
            "{id}: exit"
        );
        assert_eq!(
            output.stdout,
            decode(&case["go"]["stdout_b64"]),
            "{id}: stdout"
        );
        assert_eq!(
            output.stderr,
            decode(&case["go"]["stderr_b64"]),
            "{id}: stderr"
        );
    }
}
