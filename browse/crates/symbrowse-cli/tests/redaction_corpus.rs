#![deny(unsafe_code)]

use std::{
    fs,
    process::{Command, Output},
};

use serde_json::Value;
use symbrowse_core::journal::{Entry, Redactor, Store};
use symbrowse_daemon::{redact_args, redact_env, redact_json, redact_str};
use symbrowse_mcp::ToolError;

const CORPUS: &[u8] = include_bytes!("../../../testdata/port/security/redaction-corpus.json");

fn corpus() -> Value {
    serde_json::from_slice(CORPUS).expect("decode synthetic redaction corpus")
}

fn assert_no_markers(output: &str, secrets: &[String], surface: &str) {
    for secret in secrets {
        assert!(
            !output.contains(secret),
            "{surface} leaked synthetic marker {secret}"
        );
    }
}

fn run_config(root: &std::path::Path, args: &[&str], autosave: Option<&str>) -> Output {
    let corpus = corpus();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrowse"));
    command
        .args(args)
        .env_clear()
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("user-profile"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env(
            "SYMBROWSE_CDP_ENDPOINT",
            corpus["endpoint"].as_str().unwrap(),
        )
        .env(
            "SYMBROWSE_ENCRYPTION_KEY",
            corpus["secret_values"][5].as_str().unwrap(),
        )
        .env("SYMBROWSE_LOG_LEVEL", "debug")
        .env("SYMBROWSE_LOG_FORMAT", "json");
    if let Some(value) = autosave {
        command.env("SYMBROWSE_AUTOSAVE", value);
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command.output().expect("run isolated config CLI")
}

#[test]
fn synthetic_secret_corpus_stays_out_of_cli_daemon_mcp_config_and_log_surfaces() {
    let corpus = corpus();
    let secrets: Vec<String> = corpus["secret_values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    let text = corpus["text"].as_str().unwrap();
    let json = &corpus["json"];
    let args: Vec<String> = corpus["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect();
    let env: Vec<(String, String)> = corpus["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();

    let daemon_outputs = [
        redact_str(text),
        redact_args(&args).join(" "),
        serde_json::to_string(&redact_env(&env)).unwrap(),
        redact_json(json).to_string(),
    ]
    .join("\n");
    assert_no_markers(&daemon_outputs, &secrets, "daemon redaction corpus");
    assert!(
        daemon_outputs.contains("status: 401"),
        "useful error context was lost"
    );
    assert!(daemon_outputs.contains("SAFE_VALUE"));

    let mcp_error = ToolError {
        code: "operation_failed".to_owned(),
        message: text.to_owned(),
        hint: Some(text.to_owned()),
        retryable: Some(false),
        requires_user_confirmation: None,
        resume_hint: Some(text.to_owned()),
        details: Some(json.clone()),
    };
    let mcp_output = format!("{}{}", mcp_error.display_message(), mcp_error.metadata());
    assert_no_markers(&mcp_output, &secrets, "MCP error metadata");
    assert!(mcp_output.contains("operation_failed"));

    let temp = tempfile::tempdir().expect("create isolated journal directory");
    let mut journal_redactor = Redactor::standard();
    journal_redactor.values.clone_from(&secrets);
    let store = Store::new(
        temp.path(),
        "redaction-corpus",
        journal_redactor,
        "test-time",
    )
    .expect("create isolated journal");
    store
        .append(Entry {
            command: "corpus".to_owned(),
            args: Some(json.clone()),
            reason: text.to_owned(),
            ..Entry::default()
        })
        .expect("append redacted log entry");
    let log_bytes = fs::read_to_string(store.path()).expect("read journal bytes");
    assert_no_markers(&log_bytes, &secrets, "journal log file");

    let temp = tempfile::tempdir().expect("create isolated CLI home");
    let config = run_config(temp.path(), &["config", "show", "--json"], None);
    assert!(
        config.status.success(),
        "config show failed: {}",
        String::from_utf8_lossy(&config.stderr)
    );
    let cli_output = format!(
        "{}{}",
        String::from_utf8_lossy(&config.stdout),
        String::from_utf8_lossy(&config.stderr)
    );
    assert_no_markers(&cli_output, &secrets, "CLI config/environment output");
    assert!(cli_output.contains("127.0.0.1:9222"));
    assert!(cli_output.contains("mode=active"));

    let failed = run_config(temp.path(), &["config", "show"], Some("invalid"));
    assert_eq!(
        failed.status.code(),
        Some(9),
        "invalid config must keep its error class"
    );
    let failure_output = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert_no_markers(&failure_output, &secrets, "CLI config error output");
    assert!(failure_output.contains("invalid autosave policy"));
}
