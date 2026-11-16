//! Append-only JSONL journal and secret redaction.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub const SCHEMA_VERSION: i64 = 1;
const MASK: &str = "••••";
const MAX_ENTRY_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub schema_version: i64,
    pub timestamp: String,
    pub session: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url_before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url_after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub result: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub risk_class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Redactor {
    pub keys: BTreeSet<String>,
    pub values: Vec<String>,
}
impl Redactor {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            keys: [
                "password",
                "pass",
                "secret",
                "token",
                "authorization",
                "api_key",
                "apikey",
                "cookie",
                "credential",
                "value",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            values: Vec::new(),
        }
    }
    #[must_use]
    pub fn redact(&self, value: &Value) -> Value {
        let mut redacted = self.redact_value(value);
        if !self.values.is_empty() {
            replace_values(&mut redacted, &self.values);
        }
        redacted
    }
    #[must_use]
    pub fn redact_text(&self, mut text: String) -> String {
        for value in &self.values {
            if !value.is_empty() {
                text = text.replace(value, MASK);
            }
        }
        text
    }
    fn redact_value(&self, value: &Value) -> Value {
        match value {
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, value)| {
                        if self.masks_key(key) {
                            (key.clone(), Value::String(MASK.to_owned()))
                        } else {
                            (key.clone(), self.redact_value(value))
                        }
                    })
                    .collect(),
            ),
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| self.redact_value(value))
                    .collect(),
            ),
            value => value.clone(),
        }
    }
    fn masks_key(&self, key: &str) -> bool {
        let key = key.to_ascii_lowercase();
        self.keys.iter().any(|candidate| {
            key == *candidate
                || key.ends_with(&format!("_{candidate}"))
                || key.ends_with(&format!("-{candidate}"))
        })
    }
}
/// Scrubs credential-shaped text (URL passwords, `token=` pairs) in every
/// string of a journaled argument tree.
fn scrub_strings(value: &mut Value) {
    match value {
        Value::String(text) => *text = crate::secret_text::scrub(text),
        Value::Array(items) => items.iter_mut().for_each(scrub_strings),
        Value::Object(items) => items.values_mut().for_each(scrub_strings),
        _ => {}
    }
}
/// Journal form of a page URL or origin: credential-shaped text scrubbed, and
/// the opaque origin of a blank or `data:` page (`location.origin` is the
/// string `"null"`) treated as "no URL known". The Go journal omits both URL
/// fields in that case, so the entry must not carry the literal text `null`.
fn journal_url(url: &str) -> String {
    if url == "null" {
        return String::new();
    }
    crate::secret_text::scrub(url)
}
fn replace_values(value: &mut Value, values: &[String]) {
    match value {
        Value::String(text) => {
            for secret in values {
                if !secret.is_empty() {
                    *text = text.replace(secret, MASK);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_values(item, values);
            }
        }
        Value::Object(items) => {
            for item in items.values_mut() {
                replace_values(item, values);
            }
        }
        _ => {}
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
    redactor: Redactor,
    timestamp: String,
}
impl Store {
    pub fn new(
        dir: &Path,
        session: &str,
        redactor: Redactor,
        timestamp: impl Into<String>,
    ) -> std::io::Result<Self> {
        if session.trim().is_empty()
            || session.contains('/')
            || session.contains('\\')
            || session.contains("..")
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid journal session",
            ));
        }
        fs::create_dir_all(dir)?;
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        if fs::symlink_metadata(dir)?.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "journal directory must not be a symlink",
            ));
        }
        Ok(Self {
            path: dir.join(format!("{session}.jsonl")),
            redactor,
            timestamp: timestamp.into(),
        })
    }
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn append(&self, mut entry: Entry) -> std::io::Result<Entry> {
        entry.schema_version = SCHEMA_VERSION;
        if entry.timestamp.is_empty() {
            entry.timestamp = self.timestamp.clone();
        }
        if let Some(args) = entry.args.as_mut() {
            *args = self.redactor.redact(args);
            scrub_strings(args);
        }
        // URLs are journaled verbatim by the callers; their userinfo password
        // and secret query values must not reach the file.
        entry.url_before = journal_url(&entry.url_before);
        entry.url_after = journal_url(&entry.url_after);
        entry.reason = self.redactor.redact_text(std::mem::take(&mut entry.reason));
        if !entry.result.is_empty() && !entry.result.starts_with("error") {
            entry.result = "ok".to_owned();
        }
        let mut line = serde_json::to_vec(&entry).map_err(std::io::Error::other)?;
        line.push(b'\n');
        if line.len() > MAX_ENTRY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "journal entry exceeds size limit",
            ));
        }
        let mut file = open_append(&self.path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        let result = (|| {
            file.write_all(&line)?;
            flush_to_storage(&file)
        })();
        let unlock = fs2::FileExt::unlock(&file);
        result?;
        unlock?;
        Ok(entry)
    }
    pub fn read(&self) -> std::io::Result<Vec<Entry>> {
        let file = match open_read(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut entries = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            entries.push(serde_json::from_str(&line).unwrap_or_else(|_| Entry {
                schema_version: SCHEMA_VERSION,
                command: "<corrupt>".to_owned(),
                result: "error:corrupt".to_owned(),
                args: Some(Value::String(self.redactor.redact_text(line))),
                ..Entry::default()
            }));
        }
        Ok(entries)
    }
    pub fn tail(&self, count: usize) -> std::io::Result<Vec<Entry>> {
        let entries = self.read()?;
        if count == 0 || count >= entries.len() {
            Ok(entries)
        } else {
            Ok(entries[entries.len() - count..].to_vec())
        }
    }

    /// Wait for entries appended after `after`, bounded by `timeout`.
    /// Cancellation is explicit and never turns a partial stream into success.
    pub fn watch_until(
        &self,
        after: usize,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> std::io::Result<Vec<Entry>> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(Vec::new());
            }
            let entries = self.read()?;
            if entries.len() > after {
                return Ok(entries.into_iter().skip(after).collect());
            }
            if Instant::now() >= deadline {
                return Ok(Vec::new());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// List journal sessions in deterministic order, ignoring unrelated files.
    pub fn sessions(dir: &Path) -> std::io::Result<Vec<String>> {
        let mut names = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.strip_suffix(".jsonl").map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        names.sort();
        Ok(names)
    }
}

/// Make one appended journal line durable against process and kernel crashes.
///
/// `File::sync_all` is `fcntl(F_FULLFSYNC)` on macOS: it also drains the drive
/// write cache and, on the shared NVMe, regularly stalls for ~6 ms. That cost
/// landed on every gated `fetch.url` request (#773 p95 gate). Plain `fsync(2)`
/// keeps the write ordered and visible to every later reader, as SQLite does by
/// default; only an abrupt power loss may drop the newest audit lines. Other
/// Unix systems keep `sync_all`, which already is `fsync(2)` there.
fn flush_to_storage(file: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        rustix::fs::fsync(file).map_err(std::io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

fn open_append(path: &Path) -> std::io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};
    open(
        path,
        OFlags::WRONLY | OFlags::APPEND | OFlags::CREATE | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(std::io::Error::other)
}

fn open_read(path: &Path) -> std::io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};
    open(path, OFlags::RDONLY | OFlags::NOFOLLOW, Mode::empty())
        .map(File::from)
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_nested_values_without_masking_names() {
        let mut r = Redactor::standard();
        r.values.push("fixture-secret-value".to_owned());
        let out = r.redact(&serde_json::json!({"username":"ada", "password":"fixture-secret-value", "nested":{"keep":"fixture-secret-value-x"}}));
        assert_eq!(out["password"], MASK);
        assert_eq!(out["username"], "ada");
        assert!(!out.to_string().contains("fixture-secret-value"));
    }

    #[test]
    fn store_rejects_symlinks_and_preserves_concurrent_lines() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::sync::Arc;

        let root = std::env::temp_dir().join(format!(
            "symbrowse-journal-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Arc::new(Store::new(&root, "session", Redactor::standard(), "fixed").unwrap());
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let mut workers = Vec::new();
        for index in 0..32 {
            let store = Arc::clone(&store);
            workers.push(std::thread::spawn(move || {
                store
                    .append(Entry {
                        command: format!("command-{index}"),
                        ..Entry::default()
                    })
                    .unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(store.read().unwrap().len(), 32);
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let outside = root.with_extension("outside");
        fs::write(&outside, b"untouched").unwrap();
        fs::remove_file(store.path()).unwrap();
        symlink(&outside, store.path()).unwrap();
        assert!(store.append(Entry::default()).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"untouched");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn oversized_entry_is_rejected_before_write() {
        let root =
            std::env::temp_dir().join(format!("symbrowse-journal-limit-{}", std::process::id()));
        let store = Store::new(&root, "limit", Redactor::standard(), "fixed").unwrap();
        let error = store
            .append(Entry {
                command: "x".repeat(MAX_ENTRY_BYTES),
                ..Entry::default()
            })
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(!store.path().exists());
        let _ = fs::remove_dir_all(root);
    }

    fn read_entry_line(session: &str, entry: Entry) -> String {
        let root = std::env::temp_dir().join(format!(
            "symbrowse-journal-shape-{session}-{}",
            std::process::id()
        ));
        let store = Store::new(&root, session, Redactor::standard(), "fixed").unwrap();
        store.append(entry).unwrap();
        let line = fs::read_to_string(store.path()).unwrap();
        let _ = fs::remove_dir_all(root);
        line
    }

    /// Byte shape the sealed Go oracle records for a read command on a page
    /// without a URL (`journal_tail`/`journal_tail_empty`): no `url_before`
    /// and no `url_after`, whether the daemon had no page or the page's
    /// opaque origin (`location.origin` of `about:blank`) is the text `null`.
    #[test]
    fn read_command_without_a_url_omits_both_url_fields() {
        for (label, url) in [("absent", ""), ("opaque", "null")] {
            let line = read_entry_line(
                &format!("read-{label}"),
                Entry {
                    session: "oracle".to_owned(),
                    command: "network.requests".to_owned(),
                    url_before: url.to_owned(),
                    url_after: url.to_owned(),
                    result: "ok".to_owned(),
                    risk_class: "read".to_owned(),
                    decider: "policy".to_owned(),
                    ..Entry::default()
                },
            );
            assert_eq!(
                line,
                concat!(
                    r#"{"schema_version":1,"timestamp":"fixed","session":"oracle","#,
                    r#""command":"network.requests","result":"ok","#,
                    r#""risk_class":"read","decider":"policy"}"#,
                    "\n"
                ),
                "{label}"
            );
        }
    }

    #[test]
    fn urls_are_still_scrubbed_and_kept() {
        let line = read_entry_line(
            "scrub",
            Entry {
                command: "open".to_owned(),
                url_before: "http://127.0.0.1:1/".to_owned(),
                url_after: "https://user:hunter2@example.test/a?token=abc123&q=1".to_owned(),
                ..Entry::default()
            },
        );
        assert!(
            line.contains(r#""url_before":"http://127.0.0.1:1/""#),
            "{line}"
        );
        assert!(line.contains("example.test/a"), "{line}");
        assert!(!line.contains("hunter2"), "{line}");
        assert!(!line.contains("abc123"), "{line}");
    }
}
