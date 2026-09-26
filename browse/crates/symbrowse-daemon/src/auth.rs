#![deny(unsafe_code)]

//! Symvault-backed browser login without returning credential material.

use std::{path::Path, process::Stdio, time::Duration};

use serde_json::{Map, Value, json};
use tokio::process::Command;
use zeroize::{Zeroize, Zeroizing};

use crate::{DaemonError, OperationContext, codes};
use symbrowse_engine_chrome::ChromePage;

const VAULT_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct Credentials {
    pub(crate) username: Zeroizing<String>,
    pub(crate) password: Zeroizing<String>,
}

impl Credentials {
    pub(crate) async fn resolve(
        program: &Path,
        entry: &str,
        operation: &OperationContext,
    ) -> Result<Self, DaemonError> {
        let child = Command::new(program)
            .arg("get")
            .arg(entry)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    return DaemonError {
                        code: codes::OPERATION_FAILED.into(),
                        message: "symvault is not installed; install symvault and add the credential entry (no plaintext fallback is provided)".into(),
                        ..Default::default()
                    };
                }
                vault_error(format!("could not start symvault: {error}"))
            })?;

        let timeout = VAULT_TIMEOUT.min(operation.remaining());
        let output = tokio::select! {
            output = tokio::time::timeout(timeout, child.wait_with_output()) => {
                match output {
                    Ok(Ok(output)) => output,
                    Ok(Err(error)) => return Err(vault_error(format!("symvault lookup failed: {error}"))),
                    Err(_) => return Err(vault_error("symvault lookup timed out")),
                }
            }
            _ = wait_for_operation(operation.clone()) => {
                return Err(DaemonError {
                    code: codes::OPERATION_TIMEOUT.into(),
                    message: "daemon operation was cancelled".into(),
                    ..Default::default()
                });
            }
        };

        let status = output.status;
        let raw = Zeroizing::new(output.stdout);
        if !status.success() {
            let reason = match status.code() {
                Some(2) => "vault entry was not found".to_owned(),
                Some(3) => "symvault is not initialized".to_owned(),
                Some(code) => format!("symvault exited with status {code}"),
                None => "symvault was terminated".to_owned(),
            };
            return Err(vault_error(format!(
                "symvault could not resolve requested entry: {reason}"
            )));
        }

        parse_output(&raw).map_err(|message| vault_error(format!("symvault entry: {message}")))
    }
}

pub(crate) async fn login(
    page: &ChromePage,
    url: &str,
    credentials: &mut Credentials,
    operation: &OperationContext,
) -> Result<Value, DaemonError> {
    if !url.is_empty() {
        page.open_with_timeout(url, operation.remaining())
            .await
            .map_err(|error| {
                let error = crate::runtime::navigation_error(error);
                if error.code == codes::OPERATION_TIMEOUT {
                    error
                } else {
                    runtime_error(redact(&error.message, credentials))
                }
            })?;
    }

    let fields = page
        .evaluate_script(DETECT_LOGIN_FIELDS)
        .await
        .map_err(|error| runtime_error(redact(&error.to_string(), credentials)))?;
    if fields.get("found") != Some(&Value::Bool(true)) {
        return Err(runtime_error("no login form detected on the current page"));
    }
    let username_selector = fields
        .get("username_selector")
        .and_then(Value::as_str)
        .filter(|selector| !selector.is_empty())
        .ok_or_else(|| runtime_error("login form did not provide a username selector"))?;
    let password_selector = fields
        .get("password_selector")
        .and_then(Value::as_str)
        .filter(|selector| !selector.is_empty())
        .ok_or_else(|| runtime_error("login form did not provide a password selector"))?;

    page.fill(username_selector, &credentials.username)
        .await
        .map_err(|error| runtime_error(redact(&error.to_string(), credentials)))?;
    page.fill(password_selector, &credentials.password)
        .await
        .map_err(|error| runtime_error(redact(&error.to_string(), credentials)))?;

    let url = page
        .evaluate_script("location.origin")
        .await
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    credentials.username.zeroize();
    credentials.password.zeroize();
    Ok(json!({
        "status": "logged_in",
        "url": url,
        "username_set": true,
        "password_set": true
    }))
}

fn parse_output(raw: &[u8]) -> Result<Credentials, &'static str> {
    let text = Zeroizing::new(String::from_utf8_lossy(raw).into_owned());
    let text = text.trim();
    if text.is_empty() {
        return Err("entry is empty");
    }
    if let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(text) {
        let username = object_field(&object, "username");
        let password = object_field(&object, "password");
        if let (Some(username), Some(password)) = (username, password)
            && !username.is_empty()
            && !password.is_empty()
        {
            let credentials = Credentials {
                username: Zeroizing::new(username.to_owned()),
                password: Zeroizing::new(password.to_owned()),
            };
            for value in object.values_mut() {
                if let Value::String(value) = value {
                    value.zeroize();
                }
            }
            return Ok(credentials);
        }
        for value in object.values_mut() {
            if let Value::String(value) = value {
                value.zeroize();
            }
        }
        return Err("entry has no username/password fields");
    }

    let mut username = None;
    let mut password = None;
    for line in text.lines() {
        let line = line.trim();
        let (key, value) = line
            .split_once(':')
            .or_else(|| line.split_once('='))
            .unwrap_or(("", ""));
        match key.trim().to_ascii_lowercase().as_str() {
            "username" | "user" | "login" => username = Some(value.trim()),
            "password" | "pass" | "secret" => password = Some(value.trim()),
            _ => {}
        }
    }
    match (
        username.filter(|value| !value.is_empty()),
        password.filter(|value| !value.is_empty()),
    ) {
        (Some(username), Some(password)) => Ok(Credentials {
            username: Zeroizing::new(username.to_owned()),
            password: Zeroizing::new(password.to_owned()),
        }),
        _ => Err("entry has no username/password fields"),
    }
}

fn object_field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    object.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(name)
            .then(|| value.as_str())
            .flatten()
    })
}

fn redact(message: &str, credentials: &Credentials) -> String {
    [credentials.username.as_str(), credentials.password.as_str()]
        .into_iter()
        .filter(|secret| !secret.is_empty())
        .fold(message.to_owned(), |text, secret| {
            text.replace(secret, "••••")
        })
}

fn runtime_error(message: impl Into<String>) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: message.into(),
        ..Default::default()
    }
}

fn vault_error(message: impl Into<String>) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: message.into(),
        ..Default::default()
    }
}

async fn wait_for_operation(operation: OperationContext) {
    while !operation.is_cancelled() && !operation.remaining().is_zero() {
        tokio::time::sleep(operation.remaining().min(Duration::from_millis(2))).await;
    }
}

const DETECT_LOGIN_FIELDS: &str = r#"(function(){
    const password = document.querySelector('input[type="password"]');
    if (!password) return {found: false};
    const form = password.closest('form');
    let username = null;
    const candidates = form ? form.querySelectorAll('input[type="text"], input[type="email"], input:not([type])') : [];
    for (const input of candidates) {
        if (input === password) break;
        if (input.type === 'password') continue;
        if (!input.disabled && !input.readOnly && input.offsetParent !== null) { username = input; break; }
    }
    if (!username) {
        const all = document.querySelectorAll('input[type="text"], input[type="email"], input:not([type])');
        for (const input of all) {
            if (input.disabled || input.readOnly || input.offsetParent === null) continue;
            username = input; break;
        }
    }
    if (!username) return {found: false};
    const uid = (el) => {
        if (el.id) return '#' + CSS.escape(el.id);
        const name = el.getAttribute('name');
        if (name) return 'input[name="' + CSS.escape(name) + '"]';
        return null;
    };
    const us = uid(username) || (function(){ let i = 0; for (const el of document.querySelectorAll('input')) { if (el === username) return 'input:nth-of-type(' + (i+1) + ')'; i++; } return null; })();
    const ps = uid(password) || (function(){ let i = 0; for (const el of document.querySelectorAll('input')) { if (el === password) return 'input:nth-of-type(' + (i+1) + ')'; i++; } return null; })();
    if (!us || !ps) return {found: false};
    return {found: true, username_selector: us, password_selector: ps, form_action: form ? (form.getAttribute('action') || '') : ''};
})()"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_go_supported_vault_shapes_without_echoing_values_in_errors() {
        for raw in [
            br#"{"USERNAME":"ada","Password":"fixture-secret"}"#.as_slice(),
            b"user: ada\nsecret = fixture-secret\n".as_slice(),
        ] {
            let credentials = parse_output(raw).expect("parse fixture credentials");
            assert_eq!(&*credentials.username, "ada");
            assert_eq!(&*credentials.password, "fixture-secret");
        }
        for raw in [b"".as_slice(), b"username: ada".as_slice()] {
            match parse_output(raw) {
                Err(error) => {
                    assert!(!error.contains("ada"));
                    assert!(!error.contains("fixture-secret"));
                }
                Ok(_) => panic!("incomplete fixture should be rejected"),
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_symvault_binary_is_used_and_credentials_are_redacted() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("fake vault directory");
        let executable = directory.path().join("symvault");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s' '{\"username\":\"fixture-user\",\"password\":\"fixture-secret\"}'\n",
        )
        .expect("fake symvault fixture");
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let operation = OperationContext::for_test();
        let credentials = Credentials::resolve(&executable, "fixture-entry", &operation)
            .await
            .expect("resolve fixture vault entry");
        assert_eq!(&*credentials.username, "fixture-user");
        assert_eq!(&*credentials.password, "fixture-secret");
        let redacted = redact("failed with fixture-user / fixture-secret", &credentials);
        assert_eq!(redacted, "failed with •••• / ••••");

        std::fs::write(&executable, "#!/bin/sh\nexit 2\n").expect("missing-entry fixture");
        let error = match Credentials::resolve(&executable, "fixture-entry", &operation).await {
            Ok(_) => panic!("missing entry should fail"),
            Err(error) => error,
        };
        assert!(!error.message.contains("fixture-entry"));
        assert!(error.message.contains("vault entry was not found"));
    }
}
