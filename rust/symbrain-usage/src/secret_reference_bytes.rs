//! Additive byte-valued Memory reference seam over the existing bounded runner.
//! String-valued Usage callers retain their existing admission and diagnostics.
use symbrain_core::{GoText, go_path};

use super::{CommandFailure, MAX_CREDENTIAL_FILE_BYTES, run_command_capture, secretref_timeout};

fn trim(mut bytes: &[u8]) -> &[u8] {
    loop {
        let n = (1..=4.min(bytes.len())).find(|&n| {
            std::str::from_utf8(&bytes[..n])
                .is_ok_and(|s| s.chars().count() == 1 && s.chars().all(char::is_whitespace))
        });
        match n {
            Some(n) => bytes = &bytes[n..],
            None => break,
        }
    }
    loop {
        let n = (1..=4.min(bytes.len())).find(|&n| {
            std::str::from_utf8(&bytes[bytes.len() - n..])
                .is_ok_and(|s| s.chars().count() == 1 && s.chars().all(char::is_whitespace))
        });
        match n {
            Some(n) => bytes = &bytes[..bytes.len() - n],
            None => break,
        }
    }
    bytes
}

fn env(name: &[u8]) -> Option<Vec<u8>> {
    std::env::var_os(go_path::from_bytes(name))
        .filter(|value| !value.is_empty())
        .map(|value| go_path::os_bytes(&value))
}

fn labeled(reference: &[u8], detail: GoText) -> GoText {
    let mut bytes = b"resolve ".to_vec();
    bytes.extend(reference);
    bytes.extend(b": ");
    bytes.extend(detail.as_ref());
    bytes.into()
}

fn capture(command: &str, args: &[Vec<u8>], action: &str) -> Result<Vec<u8>, GoText> {
    let timeout = secretref_timeout();
    let args: Vec<_> = args.iter().map(|arg| go_path::from_bytes(arg)).collect();
    match run_command_capture(command, &args, timeout, MAX_CREDENTIAL_FILE_BYTES) {
        Ok(bytes) => Ok(trim(&bytes).to_vec()),
        Err(CommandFailure::ExitFailed { code, stderr }) => {
            let mut bytes = code.map_or_else(
                || b"signal: killed".to_vec(),
                |code| format!("exit status {code}").into_bytes(),
            );
            let detail = trim(&stderr);
            if !detail.is_empty() {
                bytes.extend(b": ");
                bytes.extend(detail);
            }
            Err(bytes.into())
        }
        Err(failure) => {
            Err(super::command_failure_message(failure, action, command, timeout).into())
        }
    }
}

fn shared(reference: &[u8]) -> Result<Vec<u8>, GoText> {
    if let Some(path) = reference.strip_prefix(b"symvault://") {
        // Go ranges invalid UTF-8 as RuneError: it is not a control rune.
        let failure = if path.is_empty() {
            Some("empty")
        } else if path.starts_with(b"-") {
            Some("must not start with '-'")
        } else if path.contains(&0) {
            Some("contains a null byte")
        } else if GoText::from(path.to_vec())
            .unicode_lossy()
            .chars()
            .any(char::is_control)
        {
            Some("contains control characters")
        } else {
            None
        };
        if let Some(detail) = failure {
            return Err(labeled(
                reference,
                format!("invalid symvault credential path: {detail}").into(),
            ));
        }
        return capture(
            "symvault",
            &[
                b"get".to_vec(),
                b"--".to_vec(),
                path.to_vec(),
                b"--print".to_vec(),
            ],
            "symvault get",
        )
        .map_err(|error| labeled(reference, error));
    }
    if let Some(rest) = reference.strip_prefix(b"keychain://") {
        let Some(split) = rest.iter().position(|b| *b == b'/') else {
            return Err(labeled(
                reference,
                "invalid keychain reference, expected keychain://service/account".into(),
            ));
        };
        let (service, account) = (&rest[..split], &rest[split + 1..]);
        if service.is_empty() || account.is_empty() {
            return Err(labeled(
                reference,
                "invalid keychain reference, expected keychain://service/account".into(),
            ));
        }
        #[cfg(target_os = "macos")]
        return capture(
            "security",
            &[
                b"find-generic-password".to_vec(),
                b"-w".to_vec(),
                b"-s".to_vec(),
                service.to_vec(),
                b"-a".to_vec(),
                account.to_vec(),
            ],
            "keychain lookup",
        )
        .map_err(|error| labeled(reference, error));
        #[cfg(not(target_os = "macos"))]
        return Err(labeled(
            reference,
            "keychain:// references are only resolvable on macOS".into(),
        ));
    }
    let name = reference.strip_prefix(b"env://").unwrap_or(reference);
    env(name).ok_or_else(|| {
        let mut bytes = b"environment variable ".to_vec();
        bytes.extend(name);
        bytes.extend(b" is not set (reference ");
        bytes.extend(reference);
        bytes.push(b')');
        bytes.into()
    })
}

/// Resolves Memory references without converting selectors or secret bytes.
/// Vault fallback, validation precedence and timeout share the existing owner.
///
/// # Errors
/// Returns reference-only diagnostics; resolved secret values are never logged.
pub fn resolve_reference_bytes(value: &[u8], env_fallback: &[u8]) -> Result<Vec<u8>, GoText> {
    if ![
        b"symvault://".as_slice(),
        b"vault://",
        b"env://",
        b"keychain://",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
    {
        return Ok(value.to_vec());
    }
    let reference = value.strip_prefix(b"vault://").map_or_else(
        || value.to_vec(),
        |rest| [b"symvault://".as_slice(), rest].concat(),
    );
    let failure = match shared(&reference) {
        Ok(secret) if !secret.is_empty() => return Ok(secret),
        Ok(_) => GoText::from("shared resolver returned empty secret"),
        Err(error) => error,
    };
    if (value.starts_with(b"symvault://") || value.starts_with(b"vault://"))
        && !env_fallback.is_empty()
        && let Some(fallback) = env(env_fallback)
    {
        return Ok(fallback);
    }
    let mut bytes = b"secret resolution failed for ".to_vec();
    bytes.extend(reference);
    bytes.extend(b": ");
    bytes.extend(failure.as_ref());
    bytes.extend(b"; set env var ");
    bytes.extend(env_fallback);
    bytes.extend(b" as fallback or install symvault");
    Err(bytes.into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn trims_only_complete_boundary_runes_and_validates_before_lookup() {
        assert_eq!(
            super::trim(b" \xc2\xa0key\xff\xe2\x82 \t"),
            b"key\xff\xe2\x82"
        );
        assert_eq!(
            super::resolve_reference_bytes(b"literal\xff", b"").unwrap(),
            b"literal\xff"
        );
        let error = super::resolve_reference_bytes(b"vault://-\xff", b"").unwrap_err();
        assert_eq!(error.as_ref(), b"secret resolution failed for symvault://-\xff: resolve symvault://-\xff: invalid symvault credential path: must not start with '-'; set env var  as fallback or install symvault");
    }
}
