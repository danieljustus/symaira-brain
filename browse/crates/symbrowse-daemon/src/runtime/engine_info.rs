//! `engine.info`: which engine this session actually runs, which version that
//! engine reports about itself, and what it can truthfully do.
//!
//! Chrome and Firefox are started (or reused) and asked for their own version
//! (`Browser.getVersion`, BiDi `session.new`). Safari is not launched: the
//! installed application's bundle version is read instead, so reporting never
//! touches the user's Safari. An engine that cannot run here returns the typed
//! `engine_unavailable` error; no other engine or static mode is substituted.

use serde_json::{Value, json};

use super::{
    DispatchRuntime, HandlerResult, OperationContext, engine_unavailable,
    ensure_firefox_advertised, firefox_error, runtime_error,
};
use crate::DaemonError;

impl DispatchRuntime {
    pub(super) async fn engine_info(&self, operation: &OperationContext) -> HandlerResult {
        let engine = self.spec.engine.clone();
        let mode = self.spec.mode.clone();
        let (kind, version, product, capabilities) = match engine.as_str() {
            "static" => ("static".to_owned(), Value::Null, Value::Null, Value::Null),
            "chrome" => {
                self.ensure_browser(operation).await?;
                let session = self.chrome_session()?;
                let version = session.browser_version().await.map_err(runtime_error)?;
                (
                    "chrome".to_owned(),
                    json!(version.number()),
                    json!(version.product),
                    serde_json::to_value(symbrowse_engine_chrome::canonical_capabilities())
                        .map_err(runtime_error)?,
                )
            }
            "firefox" => {
                ensure_firefox_advertised(std::env::consts::OS)?;
                self.ensure_firefox_session().await?;
                let guard = self.firefox.lock().await;
                let session = guard
                    .as_ref()
                    .ok_or_else(|| runtime_error("Firefox runtime was not initialized"))?;
                (
                    "firefox".to_owned(),
                    json!(session.browser_version()),
                    json!(format!(
                        "{}/{}",
                        session.browser_name(),
                        session.browser_version()
                    )),
                    serde_json::to_value(symbrowse_engine_firefox::canonical_capabilities())
                        .map_err(runtime_error)?,
                )
            }
            "safari-attach" | "safari-bidi" => self.safari_info()?,
            other => {
                return Err(engine_unavailable(
                    other,
                    "no such engine is implemented; nothing is substituted",
                ));
            }
        };
        Ok((
            Some(json!({
                "mode": mode,
                "engine": engine,
                "kind": kind,
                "version": version,
                "product": product,
                "capabilities": capabilities,
            })),
            Vec::new(),
        ))
    }

    #[cfg(target_os = "macos")]
    fn safari_info(&self) -> Result<(String, Value, Value, Value), DaemonError> {
        let version = installed_safari_version()
            .ok_or_else(|| engine_unavailable("safari", "Safari.app was not found"))?;
        let capabilities = serde_json::to_value(
            crate::safari_runtime::SafariRuntime::planned_capabilities(&self.spec),
        )
        .map_err(runtime_error)?;
        Ok((
            "safari".to_owned(),
            json!(version),
            json!(format!("Safari/{version}")),
            capabilities,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn safari_info(&self) -> Result<(String, Value, Value, Value), DaemonError> {
        Err(engine_unavailable(
            "safari",
            "Safari engines are only available on macOS",
        ))
    }

    /// Start the owned Firefox session if it is not running yet.
    pub(super) async fn ensure_firefox_session(&self) -> Result<(), DaemonError> {
        let mut guard = self.firefox.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let executable = symbrowse_engine_firefox::resolve_firefox_executable(
            (!self.spec.executable_path.as_os_str().is_empty())
                .then_some(self.spec.executable_path.as_path()),
        )
        .map_err(firefox_error)?;
        let session = symbrowse_engine_firefox::FirefoxSession::launch(
            executable,
            self.spec.user_data_dir(),
            self.spec.operation_timeout,
        )
        .await
        .map_err(firefox_error)?;
        *guard = Some(session);
        Ok(())
    }
}

/// Version of the installed Safari application, read from its bundle without
/// launching it.
#[cfg(target_os = "macos")]
fn installed_safari_version() -> Option<String> {
    for app in [
        "/Applications/Safari.app",
        "/System/Cryptexes/App/System/Applications/Safari.app",
    ] {
        let output = std::process::Command::new("/usr/bin/defaults")
            .args([
                "read",
                &format!("{app}/Contents/Info"),
                "CFBundleShortVersionString",
            ])
            .output()
            .ok()?;
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && !version.is_empty() {
            return Some(version);
        }
    }
    None
}
