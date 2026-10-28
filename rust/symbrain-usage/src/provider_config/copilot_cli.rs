// Current Copilot CLI selection is separate from legacy apps/hosts files.
#[test]
fn current_copilot_shared_corpus() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../tests/fixtures/current_copilot_cli.json"
    ))
    .expect("shared corpus");
    assert_eq!(cases.len(), 11);
    for case in cases {
        let data = case["data"].as_str().unwrap().as_bytes();
        let account = copilot_cli_account(data).unwrap_or_default();
        assert_eq!(account, case["account"].as_str().unwrap(), "{}", case["id"]);
        let token = if account.is_empty() {
            None
        } else {
            copilot_cli_file_token(data, &account)
        };
        assert_eq!(
            token.unwrap_or_default(),
            case["token"].as_str().unwrap(),
            "{}",
            case["id"]
        );
        let calls = std::cell::Cell::new(0);
        let resolved = copilot_cli_token_with(data, |selected| {
            calls.set(calls.get() + 1);
            assert_eq!(selected, account);
            Some("fixture-keychain".into())
        });
        if account.is_empty() {
            assert!(resolved.is_none());
            assert_eq!(calls.get(), 0);
        } else if case["token"] == "" {
            assert_eq!(resolved, Some(("fixture-keychain".into(), true)));
            assert_eq!(calls.get(), 1);
            assert!(copilot_cli_token_with(data, |_| None).is_none());
        } else {
            assert_eq!(
                resolved,
                Some((case["token"].as_str().unwrap().into(), false))
            );
            assert_eq!(calls.get(), 0);
        }
        if let Some((token, keychain)) = resolved {
            let source = if keychain { "keychain" } else { "file" };
            let provider = copilot_from_resolved(Some((source.into(), token.clone())), None);
            let transport = std::sync::Arc::new(crate::FixtureTransport::new(std::collections::BTreeMap::from([
                ("copilot".into(), crate::Response {
                    status: 200, headers: std::collections::BTreeMap::new(),
                    body: br#"{"quota_snapshots":{"premium_interactions":{"has_quota":true,"entitlement":100,"remaining":75}}}"#.to_vec(),
                })
            ])));
            let report = crate::Service::with_transport(vec![provider], transport.clone()).report();
            assert!(report.providers[0].configured);
            assert!(report.providers[0].error.is_none());
            assert_eq!(
                report.providers[0].snapshot.as_ref().unwrap().meters[0]
                    .used
                    .as_deref(),
                Some("25")
            );
            assert!(
                transport.requests()[0]
                    .headers
                    .iter()
                    .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                        && value == &format!("Bearer {token}"))
            );
        }
    }
}
const COPILOT_CLI_LOGIN_ERROR: &str = "Copilot CLI login detected but selected account token is unavailable — unlock the macOS Keychain or sign in again with the Copilot CLI";

#[derive(Deserialize, PartialEq)]
struct CopilotCLIUser {
    host: String,
    login: String,
}

fn copilot_cli_account(data: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default, rename = "loggedInUsers")]
        users: Vec<CopilotCLIUser>,
        #[serde(rename = "lastLoggedInUser")]
        last: Option<CopilotCLIUser>,
    }
    let config: Config = serde_json::from_slice(&strip_copilot_jsonc(data)?).ok()?;
    let selected = if let Some(last) = config.last {
        config.users.into_iter().find(|user| user == &last)?
    } else if config.users.len() == 1 {
        config.users.into_iter().next()?
    } else {
        return None;
    };
    if selected.host != "https://github.com"
        || selected.login.is_empty()
        || selected.login.len() > 39
        || !selected
            .login
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return None;
    }
    Some(format!("{}:{}", selected.host, selected.login))
}

fn copilot_cli_file_token(data: &[u8], account: &str) -> Option<String> {
    let config: serde_json::Value = serde_json::from_slice(&strip_copilot_jsonc(data)?).ok()?;
    config
        .get("authTokens")
        .and_then(|entries| entries.get(format!("{account}:github")))
        .and_then(|entry| entry.get("token"))
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .or_else(|| {
            config
                .get("copilotTokens")
                .and_then(|entries| entries.get(account))
                .and_then(serde_json::Value::as_str)
                .filter(|token| !token.is_empty())
        })
        .map(str::to_owned)
}

fn copilot_cli_token_with(
    data: &[u8],
    read_keychain: impl Fn(&str) -> Option<String>,
) -> Option<(String, bool)> {
    let account = copilot_cli_account(data)?;
    if let Some(token) = copilot_cli_file_token(data, &account) {
        return Some((token, false));
    }
    read_keychain(&account).map(|token| (token, true))
}

fn copilot_cli_has_login(data: &[u8]) -> bool {
    let Some(clean) = strip_copilot_jsonc(data) else {
        return true;
    };
    let Ok(config) = serde_json::from_slice::<serde_json::Value>(&clean) else {
        return true;
    };
    if let Some(users) = config.get("loggedInUsers") {
        if !users.is_null() && !users.is_array() {
            return true;
        }
        if users.as_array().is_some_and(|users| !users.is_empty()) {
            return true;
        }
    }
    config
        .get("lastLoggedInUser")
        .is_some_and(|last| !last.is_null())
}

fn copilot_cli_path() -> PathBuf {
    home().join(".copilot/config.json")
}

#[cfg(target_os = "macos")]
fn copilot_keychain(account: &str) -> Option<String> {
    let blob = bounded_command_stdout(
        "/usr/bin/security",
        &[
            "find-generic-password",
            "-w",
            "-s",
            "copilot-cli",
            "-a",
            account,
        ],
        CLAUDE_KEYCHAIN_TIMEOUT,
        MAX_CREDENTIAL_FILE_BYTES,
    )?;
    let token = String::from_utf8(blob).ok()?.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

#[cfg(not(target_os = "macos"))]
fn copilot_keychain(_: &str) -> Option<String> {
    None
}

fn strip_copilot_jsonc(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = data
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .unwrap_or(data)
        .to_vec();
    let (mut quoted, mut escaped) = (false, false);
    let mut i = 0;
    while i < out.len() {
        let c = out[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                quoted = false;
            }
        } else if c == b'"' {
            quoted = true;
        } else if c == b'/' && i + 1 < out.len() {
            if out[i + 1] == b'/' {
                while i < out.len() && out[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            } else if out[i + 1] == b'*' {
                let end = out[i + 2..].windows(2).position(|p| p == b"*/")? + i + 4;
                out[i..end].fill(b' ');
                i = end - 1;
            }
        }
        i += 1;
    }
    (quoted, escaped) = (false, false);
    for i in 0..out.len() {
        let c = out[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                quoted = false;
            }
            continue;
        }
        if c == b'"' {
            quoted = true;
            continue;
        }
        if c == b','
            && out[i + 1..]
                .iter()
                .find(|c| !c.is_ascii_whitespace())
                .is_some_and(|c| *c == b'}' || *c == b']')
        {
            out[i] = b' ';
        }
    }
    Some(out)
}
