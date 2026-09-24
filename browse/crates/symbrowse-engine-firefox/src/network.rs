use crate::{FirefoxError, FirefoxSession};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

impl FirefoxSession {
    /// Set the deadline used by subsequent BiDi commands.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Return Firefox's loopback-only remote debugging endpoint.
    pub fn remote_endpoint(&self) -> std::net::SocketAddr {
        self.endpoint
    }

    /// Subscribe to completed network responses for the current context.
    pub async fn start_response_capture(&mut self) -> Result<(), FirefoxError> {
        self.bidi.events.clear();
        self.bidi.events_overflowed = false;
        self.bidi
            .command(
                "session.subscribe",
                json!({"events":["network.responseCompleted"],"contexts":[self.context]}),
                self.timeout,
            )
            .await?;
        Ok(())
    }

    /// Drain completed response events after a BiDi command barrier.
    pub async fn take_response_capture(&mut self) -> Result<Vec<Value>, FirefoxError> {
        self.bidi
            .command("browsingContext.getTree", json!({}), self.timeout)
            .await?;
        if self.bidi.events_overflowed {
            self.bidi.events.clear();
            self.bidi.events_overflowed = false;
            return Err(FirefoxError::Driver(
                "Firefox response capture exceeded its 256-event bound".into(),
            ));
        }
        Ok(std::mem::take(&mut self.bidi.events))
    }

    /// Route browser downloads into an existing caller-owned directory.
    pub async fn allow_downloads(&mut self, destination: &Path) -> Result<(), FirefoxError> {
        if !destination.is_dir() {
            return Err(FirefoxError::Driver(format!(
                "Firefox download directory is unavailable: {}",
                destination.display()
            )));
        }
        let destination = destination.to_str().ok_or_else(|| {
            FirefoxError::Driver("Firefox download directory is not valid Unicode".into())
        })?;
        self.bidi
            .command(
                "browser.setDownloadBehavior",
                json!({"downloadBehavior":{"type":"allowed","destinationFolder":destination}}),
                self.timeout,
            )
            .await?;
        Ok(())
    }
}
