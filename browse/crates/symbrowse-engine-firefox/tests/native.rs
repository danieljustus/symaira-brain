//! ENG-010: native Firefox WebDriver BiDi corpus at the adapter level
//! (lifecycle, navigation, cookies/storage, JavaScript, response capture,
//! timeout, typed unsupported/unavailable errors and cleanup). The loopback
//! fixture is the one shared with the daemon corpus (ENG-009), included from
//! its single source file. Native cases run under `SYMBROWSE_E2E=1`; the typed
//! error cases need no browser and always run.
#![cfg(unix)]
#![deny(unsafe_code)]

#[path = "../../symbrowse-daemon/tests/common/corpus_fixture.rs"]
mod corpus_fixture;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use serde_json::json;
use symbrowse_engine_firefox::{FirefoxError, FirefoxSession, resolve_firefox_executable};

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn profile(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "symbrowse-firefox-bidi-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn owned_processes(marker: &Path) -> Vec<String> {
    let marker = marker.to_string_lossy().into_owned();
    let table = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&table.stdout)
        .lines()
        .filter(|line| line.contains(&marker))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn missing_executable_is_the_typed_unavailable_error() {
    let missing = Path::new("/definitely/not/firefox");
    assert!(matches!(
        resolve_firefox_executable(Some(missing)),
        Err(FirefoxError::Unavailable(_))
    ));
    // Launching a path that is not a runnable Firefox never falls back.
    let error = FirefoxSession::launch(
        missing.to_path_buf(),
        profile("missing"),
        Duration::from_secs(5),
    )
    .await
    .err()
    .expect("launch must fail");
    assert!(matches!(error, FirefoxError::Unavailable(_)), "{error:?}");
}

#[tokio::test]
async fn bidi_lifecycle_navigation_cookies_storage_capture_timeout_and_cleanup() {
    if !enabled() {
        return;
    }
    let executable = resolve_firefox_executable(
        std::env::var_os("SYMBROWSE_FIREFOX_EXECUTABLE")
            .as_deref()
            .map(Path::new),
    )
    .expect("native Firefox corpus needs a local Firefox");
    let fixture = corpus_fixture::serve();
    let profile = profile("corpus");
    let mut session =
        FirefoxSession::launch(executable.clone(), profile.clone(), Duration::from_secs(20))
            .await
            .expect("launch Firefox over BiDi");

    // Lifecycle: the session reports the engine's own name and version.
    assert_eq!(session.browser_name(), "firefox");
    let version = session.browser_version().to_owned();
    let output = Command::new(&executable)
        .arg("--version")
        .output()
        .expect("--version");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&version),
        "BiDi version {version} vs {:?}",
        output.stdout
    );
    println!("PARITY_CASE ENG-010.lifecycle-version PASS");

    // Navigation through a redirect; JavaScript state from the page's script.
    let navigation = session
        .navigate(&fixture.url("/redirect"))
        .await
        .expect("navigate");
    assert!(navigation.url.ends_with("/landing"), "{}", navigation.url);
    let ready = session
        .evaluate("document.body.dataset.ready")
        .await
        .expect("evaluate");
    assert_eq!(ready.value, Some(json!("yes")));
    let sum = session.evaluate("6 * 7").await.expect("evaluate");
    assert_eq!(sum.value, Some(json!(42)));
    let thrown = session
        .evaluate("throw new Error('probe')")
        .await
        .expect("evaluate");
    assert!(!thrown.exception_text.is_empty(), "{thrown:?}");
    println!("PARITY_CASE ENG-010.navigation-javascript PASS");

    // Cookies through the BiDi storage module: HTTP Set-Cookie and script
    // cookie are present; one written over BiDi is sent by Firefox itself.
    let cookies = session.cookies().await.expect("cookies");
    let names: Vec<&str> = cookies["cookies"]
        .as_array()
        .expect("cookie list")
        .iter()
        .filter_map(|cookie| cookie["name"].as_str())
        .collect();
    assert!(
        names.contains(&"session") && names.contains(&"js"),
        "{names:?}"
    );
    session
        .set_cookie(json!({
            "name": "manual",
            "value": {"type": "string", "value": "1"},
            "domain": "127.0.0.1",
            "path": "/",
        }))
        .await
        .expect("set cookie");
    session.navigate(&fixture.url("/echo")).await.expect("echo");
    let echoed = session
        .evaluate("document.body.innerText")
        .await
        .expect("echo text")
        .value
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    for expected in ["session=abc", "js=1", "manual=1", "Firefox/"] {
        assert!(echoed.contains(expected), "{expected} missing: {echoed}");
    }
    session
        .delete_cookies("manual")
        .await
        .expect("delete cookie");
    let cookies = session.cookies().await.expect("cookies");
    assert!(
        cookies["cookies"]
            .as_array()
            .expect("cookie list")
            .iter()
            .all(|cookie| cookie["name"] != "manual"),
        "{cookies}"
    );
    println!("PARITY_CASE ENG-010.cookies PASS");

    // Storage state written by the page script is readable in-page.
    session
        .navigate(&fixture.url("/landing"))
        .await
        .expect("landing");
    let storage = session
        .evaluate("localStorage.getItem('k') + ':' + sessionStorage.getItem('s')")
        .await
        .expect("storage");
    assert_eq!(storage.value, Some(json!("v:1")));
    println!("PARITY_CASE ENG-010.storage PASS");

    // Response capture over network.responseCompleted.
    session.start_network_capture().await.expect("capture");
    session.navigate(&fixture.url("/json")).await.expect("json");
    let responses = session.network_responses().await.expect("responses");
    assert!(
        responses.iter().any(|response| {
            response["url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/json"))
                && response["status"] == 200
                && response["mime_type"]
                    .as_str()
                    .is_some_and(|mime| mime.contains("json"))
        }),
        "{responses:?}"
    );
    println!("PARITY_CASE ENG-010.response-capture PASS");

    // Unsupported operations fail explicitly and are typed.
    assert!(matches!(
        session.interact("press", "body", Some("Enter")).await,
        Err(FirefoxError::Unsupported { .. })
    ));
    assert!(matches!(
        session.screenshot("gif").await,
        Err(FirefoxError::Unsupported { .. })
    ));
    assert!(matches!(
        session.navigate("file:///etc/hosts").await,
        Err(FirefoxError::InvalidTarget(_))
    ));
    println!("PARITY_CASE ENG-010.typed-unsupported PASS");

    session.close().await.expect("close");
    drop(session);

    // A second session proves the timeout is typed and still cleaned up.
    let profile_timeout = self::profile("timeout");
    let mut slow =
        FirefoxSession::launch(executable, profile_timeout.clone(), Duration::from_secs(4))
            .await
            .expect("launch second Firefox");
    let started = Instant::now();
    let hung = slow.navigate(&fixture.url("/hang")).await;
    assert!(
        matches!(hung, Err(FirefoxError::Timeout { .. })),
        "{hung:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(12),
        "{:?}",
        started.elapsed()
    );
    println!("PARITY_CASE ENG-010.timeout PASS");
    slow.close().await.expect("close second");
    drop(slow);

    // Cleanup: no Firefox process of this corpus survives its sessions.
    for marker in [&profile, &profile_timeout] {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let leftover = owned_processes(marker);
            if leftover.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Firefox survived close: {leftover:?}"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let _ = std::fs::remove_dir_all(marker);
    }
    println!("PARITY_CASE ENG-010.cleanup PASS");
}
