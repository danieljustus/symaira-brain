//! ENG-002: the switches Chrome actually received, read back from the running
//! browser over CDP (`Browser.getBrowserCommandLine`). Opt-in like the other
//! real-Chrome tests (`SYMBROWSE_E2E=1`).

use std::{path::PathBuf, time::Duration};

use chromiumoxide::cdp::browser_protocol::browser::GetBrowserCommandLineParams;
use symbrowse_engine_chrome::{BrowserMode, ChromeSession};

fn chrome_executable() -> PathBuf {
    symbrowse_engine_chrome::discover_chrome_executable().expect("Chrome executable")
}

async fn command_line(disable_webrtc: bool) -> Vec<String> {
    let profile = std::env::temp_dir().join(format!(
        "symbrowse-eng002-{}-{disable_webrtc}",
        std::process::id()
    ));
    let session = ChromeSession::connect(
        BrowserMode::Launch {
            executable: chrome_executable(),
            user_data_dir: profile.clone(),
            headless: true,
            disable_webrtc,
        },
        Duration::from_secs(20),
    )
    .await
    .expect("launch Chrome");
    let page = session.new_page("about:blank").await.expect("blank page");
    let arguments = page
        .raw()
        .execute(GetBrowserCommandLineParams::default())
        .await
        .expect("Browser.getBrowserCommandLine")
        .result
        .arguments;
    drop(page);
    drop(session);
    let _ = std::fs::remove_dir_all(profile);
    arguments
}

#[tokio::test]
async fn chrome_receives_the_go_privacy_and_allowlist_switches() {
    if std::env::var_os("SYMBROWSE_E2E").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let with_allowlist = command_line(true).await;
    for switch in [
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-domain-reliability",
        "--disable-sync",
        "--disable-webrtc",
    ] {
        assert!(
            with_allowlist.iter().any(|arg| arg == switch),
            "{switch} missing from {with_allowlist:?}"
        );
    }
    let features = with_allowlist
        .iter()
        .find_map(|arg| arg.strip_prefix("--disable-features="))
        .expect("--disable-features");
    for feature in ["OptimizationHints", "MediaRouter"] {
        assert!(
            features.split(',').any(|value| value == feature),
            "{feature} in {features}"
        );
    }
    assert!(with_allowlist.iter().all(|arg| !arg.starts_with("----")));

    let without_allowlist = command_line(false).await;
    assert!(
        !without_allowlist
            .iter()
            .any(|arg| arg == "--disable-webrtc")
    );
}
