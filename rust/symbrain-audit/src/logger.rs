use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};

use crate::model::{Classification, Config, Entry, Exposure};
use crate::redact::redact_args;
use crate::sink::{Sink, go_json};

struct State {
    sink: Sink,
    degraded: bool,
    dropped: u64,
}

/// Concurrent redacting audit logger backed by a hash-chained JSONL sink.
pub struct Logger {
    path: Option<PathBuf>,
    profile: String,
    session: String,
    config: Config,
    state: Option<Mutex<State>>,
}

impl Logger {
    /// Opens the profile log under the process XDG audit directory.
    ///
    /// # Errors
    /// Returns an error when the XDG path cannot be resolved or the sink cannot be opened.
    pub fn open(profile: &str, config: Config) -> io::Result<Self> {
        if !config.enabled {
            return Ok(Self::disabled(config));
        }
        let dir = symbrain_core::xdg::audit_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "audit: resolve audit dir: home directory unavailable",
            )
        })?;
        Self::open_in(&dir, profile, config)
    }

    /// Opens the profile log in an explicit directory.
    ///
    /// # Errors
    /// Returns an error when the private directory or log file cannot be opened.
    pub fn open_in(dir: &Path, profile: &str, config: Config) -> io::Result<Self> {
        let session = now();
        Self::open_in_with_session(dir, profile, config, &session)
    }

    /// Opens with a fixed session identifier for deterministic contract tests.
    ///
    /// # Errors
    /// Returns an error when the log cannot be opened or its retained chain is invalid.
    pub fn open_in_with_session(
        dir: &Path,
        profile: &str,
        config: Config,
        session: &str,
    ) -> io::Result<Self> {
        if !config.enabled {
            return Ok(Self::disabled(config));
        }
        let path = dir.join(format!("{profile}.jsonl"));
        let sink = Sink::open(&path)?;
        Ok(Self {
            path: Some(path),
            profile: profile.to_string(),
            session: session.to_string(),
            config,
            state: Some(Mutex::new(State {
                sink,
                degraded: false,
                dropped: 0,
            })),
        })
    }

    fn disabled(config: Config) -> Self {
        Self {
            path: None,
            profile: String::new(),
            session: String::new(),
            config,
            state: None,
        }
    }

    /// Records one routed call using the current UTC timestamp.
    #[allow(clippy::too_many_arguments)]
    pub fn log(
        &self,
        server: &str,
        tool: &str,
        args: &[u8],
        duration: Duration,
        status: &str,
        exposure: &Exposure,
        classification: Option<&Classification>,
    ) {
        self.log_at(
            &now(),
            server,
            tool,
            args,
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
            status,
            exposure,
            classification,
        );
    }

    /// Records one routed call with a fixed timestamp and duration.
    #[allow(clippy::too_many_arguments)]
    pub fn log_at(
        &self,
        timestamp: &str,
        server: &str,
        tool: &str,
        args: &[u8],
        duration_ms: i64,
        status: &str,
        exposure: &Exposure,
        classification: Option<&Classification>,
    ) {
        let Some(state) = &self.state else {
            return;
        };
        let (arg_keys, arg_values) = redact_args(server, tool, args, self.config.verbose);
        let classification = classification.cloned().unwrap_or_default();
        let entry = Entry {
            timestamp: timestamp.to_string(),
            session_id: self.session.clone(),
            profile: self.profile.clone(),
            server: server.to_string(),
            tool: tool.to_string(),
            duration_ms,
            status: status.to_string(),
            category: classification.category,
            retryable: classification.retryable,
            arg_keys,
            arg_values,
            access_class: exposure.access_class.clone(),
            access_source: exposure.access_source.clone(),
            ..Entry::default()
        };
        append_entry(state, &entry);
    }

    /// Records a backend omitted while the session catalog was built.
    pub fn log_degradation(&self, server: &str, reason: &str, level: &str) {
        self.log_degradation_at(&now(), server, reason, level);
    }

    /// Records a degradation with a fixed timestamp.
    pub fn log_degradation_at(&self, timestamp: &str, server: &str, reason: &str, level: &str) {
        let Some(state) = &self.state else {
            return;
        };
        let entry = Entry {
            timestamp: timestamp.to_string(),
            session_id: self.session.clone(),
            profile: self.profile.clone(),
            server: server.to_string(),
            status: "degraded".to_string(),
            retryable: false,
            reason: reason.to_string(),
            level: level.to_string(),
            ..Entry::default()
        };
        append_entry(state, &entry);
    }

    /// Reports whether any write was dropped.
    #[must_use]
    pub fn degraded(&self) -> bool {
        self.state.as_ref().is_some_and(|state| {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .degraded
        })
    }

    /// Returns the total number of dropped entries.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.state.as_ref().map_or(0, |state| {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .dropped
        })
    }

    /// Flushes and closes the backing sink.
    ///
    /// # Errors
    /// Returns an error when buffered audit bytes cannot be flushed.
    pub fn close(&self) -> io::Result<()> {
        let Some(state) = &self.state else {
            return Ok(());
        };
        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sink
            .close()
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

fn append_entry(state: &Mutex<State>, entry: &Entry) {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = go_json(entry).and_then(|data| state.sink.append(&data));
    if result.is_err() {
        state.degraded = true;
        state.dropped += 1;
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::Deserialize;
    use tempfile::tempdir;

    use super::*;

    #[derive(Deserialize)]
    struct Envelope {
        d: String,
    }

    #[test]
    fn fixed_entry_matches_go_field_order_and_redaction() {
        let dir = tempdir().expect("tempdir");
        let logger = Logger::open_in_with_session(
            dir.path(),
            "personal",
            Config {
                enabled: true,
                verbose: true,
            },
            "session-1",
        )
        .expect("open");
        logger.log_at(
            "2026-01-01T00:00:00Z",
            "memory",
            "memory_search",
            br#"{"content":"private","query":"term"}"#,
            42,
            "error",
            &Exposure::default(),
            Some(&Classification {
                category: "timeout".to_string(),
                retryable: true,
            }),
        );
        logger.close().expect("close");
        let line = fs::read_to_string(logger.path().expect("path")).expect("read");
        let envelope: Envelope = serde_json::from_str(line.trim()).expect("envelope");
        assert_eq!(
            envelope.d,
            r#"{"timestamp":"2026-01-01T00:00:00Z","session_id":"session-1","profile":"personal","server":"memory","tool":"memory_search","duration_ms":42,"status":"error","category":"timeout","retryable":true,"arg_keys":"content,query","arg_values":"content=[redacted],query=term"}"#
        );
    }

    #[test]
    fn disabled_logger_has_no_side_effects() {
        let logger = Logger::open_in(tempdir().expect("tempdir").path(), "p", Config::default())
            .expect("disabled open");
        logger.log(
            "memory",
            "search",
            b"{}",
            Duration::ZERO,
            "ok",
            &Exposure::default(),
            None,
        );
        assert!(logger.path().is_none());
        assert!(!logger.degraded());
    }
}
