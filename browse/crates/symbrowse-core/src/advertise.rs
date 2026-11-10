//! Which browser engines the product advertises on which operating system.
//!
//! An engine is advertised on a platform only after its own native gate has
//! passed there (RUST-013, `docs/rust-port/work-items.json`). Advertising is
//! about what is offered to users (`--engine` validation, capability
//! handshakes, docs); an explicitly requested engine that is not advertised
//! still fails with a typed error and never becomes another engine.

use crate::config::BrowserEngine;

/// Safari needs Remote Automation on the user's real Safari, so its native gate
/// is a maintainer step. Flip this only together with a recorded gate receipt.
pub const SAFARI_NATIVE_GATE_PASSED: bool = false;

/// Operating systems whose native Firefox BiDi gate has passed
/// (`.github/workflows/browse-firefox-native.yml`: linux-amd64, darwin-arm64).
const FIREFOX_GATED_OS: [&str; 2] = ["macos", "linux"];

/// Whether `engine` is advertised on `os` (a `std::env::consts::OS` value).
#[must_use]
pub fn is_advertised(engine: BrowserEngine, os: &str) -> bool {
    match engine {
        BrowserEngine::Chrome => true,
        BrowserEngine::Firefox => FIREFOX_GATED_OS.contains(&os),
        BrowserEngine::Safari => SAFARI_NATIVE_GATE_PASSED && os == "macos",
    }
}

/// Engines advertised on `os`, in stable order.
#[must_use]
pub fn advertised_engines(os: &str) -> Vec<BrowserEngine> {
    [
        BrowserEngine::Chrome,
        BrowserEngine::Safari,
        BrowserEngine::Firefox,
    ]
    .into_iter()
    .filter(|engine| is_advertised(*engine, os))
    .collect()
}

/// Engine names (as accepted by `--engine`) that map to an engine kind.
#[must_use]
pub fn engine_kind(name: &str) -> Option<BrowserEngine> {
    match name {
        "chrome" => Some(BrowserEngine::Chrome),
        "safari" | "safari-attach" | "safari-bidi" => Some(BrowserEngine::Safari),
        "firefox" => Some(BrowserEngine::Firefox),
        _ => None,
    }
}

/// Typed refusal for an engine that is requested but not advertised on `os`.
#[must_use]
pub fn not_advertised_message(name: &str, os: &str) -> String {
    format!(
        "engine_not_advertised: engine {name:?} is not advertised on {os}: its native gate has not passed there"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firefox_is_advertised_only_on_macos_and_linux() {
        for (os, expected) in [
            ("macos", true),
            ("linux", true),
            ("windows", false),
            ("freebsd", false),
        ] {
            assert_eq!(is_advertised(BrowserEngine::Firefox, os), expected, "{os}");
        }
    }

    #[test]
    fn safari_is_never_advertised_before_its_gate() {
        for os in ["macos", "linux", "windows"] {
            assert!(!is_advertised(BrowserEngine::Safari, os), "{os}");
            assert!(!advertised_engines(os).contains(&BrowserEngine::Safari));
        }
        for name in ["safari", "safari-attach", "safari-bidi"] {
            assert_eq!(engine_kind(name), Some(BrowserEngine::Safari));
        }
    }

    #[test]
    fn chrome_stays_advertised_everywhere() {
        for os in ["macos", "linux", "windows"] {
            assert!(advertised_engines(os).contains(&BrowserEngine::Chrome));
        }
    }

    #[test]
    fn advertised_lists_are_exact() {
        assert_eq!(
            advertised_engines("macos"),
            [BrowserEngine::Chrome, BrowserEngine::Firefox]
        );
        assert_eq!(
            advertised_engines("linux"),
            [BrowserEngine::Chrome, BrowserEngine::Firefox]
        );
        assert_eq!(advertised_engines("windows"), [BrowserEngine::Chrome]);
    }
}
