//! Memory owns primary-key provisioning and persisted rotation state.
use std::{io::Write, path::Path};

use symbrain_core::GoText;

use crate::{SecretOptions, startup_fallback, startup_fs};

pub(super) struct KeyState {
    _primary: Vec<u8>,
    _fallback: Vec<startup_fallback::Entry>,
}

pub(super) fn initialize(
    options: SecretOptions,
    warnings: &mut dyn Write,
) -> Result<KeyState, GoText> {
    let mut primary = options.primary;
    if primary.is_empty() {
        let path = options.path.as_ref().map_err(Clone::clone)?;
        if let Ok(bytes) = startup_fs::read_file(path) {
            primary = trim_space(&bytes).to_vec();
        }
        if primary.is_empty() {
            primary = generate(path)
                .map_err(|error| error.with_prefix("failed to generate JWT secret: "))?;
        }
    }
    let entries = match &options.path {
        Ok(path) => startup_fallback::load(path, &primary, warnings),
        Err(_) => Vec::new(), // Go fallbackSecretsPath returns empty on resolution failure.
    };
    Ok(KeyState {
        _primary: primary,
        _fallback: entries,
    })
}

fn generate(path: &Path) -> Result<Vec<u8>, GoText> {
    use std::fmt::Write;
    let mut random = [0; 32];
    getrandom::fill(&mut random).map_err(|error| GoText::from(error.to_string()))?;
    let mut secret = String::with_capacity(64);
    for byte in random {
        write!(secret, "{byte:02x}").expect("String write cannot fail");
    }
    startup_fs::mkdir_private(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    startup_fs::write_private(path, format!("{secret}\n").as_bytes())?;
    Ok(secret.into_bytes())
}

// Go TrimSpace stops at an invalid rune; trim only complete boundary runes.
fn trim_space(mut bytes: &[u8]) -> &[u8] {
    loop {
        let count = (1..=4.min(bytes.len())).find(|&n| {
            std::str::from_utf8(&bytes[..n])
                .is_ok_and(|s| s.chars().count() == 1 && s.chars().all(char::is_whitespace))
        });
        match count {
            Some(n) => bytes = &bytes[n..],
            None => break,
        }
    }
    loop {
        let count = (1..=4.min(bytes.len())).find(|&n| {
            std::str::from_utf8(&bytes[bytes.len() - n..])
                .is_ok_and(|s| s.chars().count() == 1 && s.chars().all(char::is_whitespace))
        });
        match count {
            Some(n) => bytes = &bytes[..bytes.len() - n],
            None => break,
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn trims_complete_runes_without_repairing_secret_bytes() {
        assert_eq!(
            super::trim_space(b" \xe2\x80\x83\xff\xe2\x82 \t"),
            b"\xff\xe2\x82"
        );
        assert_eq!(super::trim_space(b" \xc2\xa0key\xc2\xa0\n"), b"key");
    }
}
