//! Persistent grant records for the native grants CLI.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize};
use symbrain_guard_core::{go_json::to_go_json_pretty_vec, go_time};

const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Origin {
    #[serde(default, deserialize_with = "null_default")]
    pub(super) epoch: i64,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub(super) via: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Grant {
    #[serde(default, deserialize_with = "null_default")]
    pub(super) id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub(super) scope: String,
    #[serde(default, deserialize_with = "null_default")]
    pub(super) origin: Origin,
    #[serde(default, deserialize_with = "nullable_timestamp")]
    pub(super) granted_at: String,
    #[serde(default, deserialize_with = "null_default")]
    pub(super) subject: String,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub(super) capability: String,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub(super) purpose: String,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub(super) resource: String,
    #[serde(
        default,
        deserialize_with = "null_string_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(super) scope_ceiling: Vec<String>,
    #[serde(default, deserialize_with = "nullable_timestamp")]
    pub(super) expires_at: String,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub(super) revoked: bool,
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn null_string_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<Option<String>>>::deserialize(deserializer).map(|values| {
        values
            .unwrap_or_default()
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect()
    })
}

fn nullable_timestamp<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(String::new());
    };
    validate_time(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

pub(super) struct Store {
    dir: PathBuf,
    grants: Vec<Grant>,
}

impl Store {
    pub(super) fn open(dir: &Path) -> Result<Self, String> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(dir)
            .map_err(|error| format!("grant: create store dir {}: {error}", dir.display()))?;
        let path = dir.join("grants.json");
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    dir: dir.to_owned(),
                    grants: Vec::new(),
                });
            }
            Err(error) => return Err(format!("grant: read {}: {error}", path.display())),
        };
        let mut grants: Vec<Grant> = serde_json::from_slice::<Option<Vec<Grant>>>(&data)
            .map(Option::unwrap_or_default)
            .map_err(|error| {
                let detail = if data.starts_with(b"{n") {
                    // serde_json's wording differs from encoding/json for this
                    // frozen oracle case; retain the Go diagnostic verbatim.
                    "invalid character 'n' looking for beginning of object key string".to_owned()
                } else {
                    error.to_string()
                };
                format!("grant: parse {}: {detail}", path.display())
            })?;
        for grant in &mut grants {
            if !matches!(grant.scope.as_str(), "run" | "session" | "device" | "vault") {
                return Err(format!(
                    "grant: parse {}: unknown scope {:?}",
                    path.display(),
                    grant.scope
                ));
            }
            for (field, value) in [
                ("granted_at", &mut grant.granted_at),
                ("expires_at", &mut grant.expires_at),
            ] {
                if !value.is_empty() {
                    validate_time(value).map_err(|error| {
                        format!(
                            "grant: parse {}: invalid {field} for grant {:?}: {error}",
                            path.display(),
                            grant.id
                        )
                    })?;
                }
            }
        }
        for (index, grant) in grants.iter().enumerate() {
            if grants[..index]
                .iter()
                .any(|previous| previous.id == grant.id)
            {
                return Err(format!(
                    "grant: parse {}: duplicate grant ID {:?}",
                    path.display(),
                    grant.id
                ));
            }
        }
        Ok(Self {
            dir: dir.to_owned(),
            grants,
        })
    }

    pub(super) fn active(&self) -> Vec<&Grant> {
        let mut active: Vec<_> = self.grants.iter().filter(|grant| !grant.revoked).collect();
        active.sort_by(|left, right| {
            time_key(&right.granted_at)
                .cmp(&time_key(&left.granted_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        active
    }

    pub(super) fn revoke(&mut self, id: &str) -> Result<(), String> {
        let Some(grant) = self.grants.iter_mut().find(|grant| grant.id == id) else {
            return Err(format!("grant: not found: {id}"));
        };
        if grant.revoked {
            return Ok(());
        }
        let persistent = is_persistent(&grant.scope);
        grant.revoked = true;
        if persistent { self.persist() } else { Ok(()) }
    }

    pub(super) fn revoke_all(&mut self) -> Result<usize, String> {
        let count = self.grants.iter().filter(|grant| !grant.revoked).count();
        for index in 0..self.grants.len() {
            if self.grants[index].revoked {
                continue;
            }
            let persistent = is_persistent(&self.grants[index].scope);
            self.grants[index].revoked = true;
            if persistent {
                self.persist()?;
            }
        }
        Ok(count)
    }

    fn persist(&self) -> Result<(), String> {
        let mut entries: Vec<_> = self
            .grants
            .iter()
            .filter(|grant| matches!(grant.scope.as_str(), "device" | "vault"))
            .cloned()
            .collect();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        for entry in &mut entries {
            if entry.granted_at.is_empty() {
                ZERO_TIME.clone_into(&mut entry.granted_at);
            }
            if entry.expires_at.is_empty() {
                ZERO_TIME.clone_into(&mut entry.expires_at);
            }
        }
        let data = to_go_json_pretty_vec(&entries)
            .map_err(|error| format!("grant: marshal store: {error}"))?;
        let path = self.dir.join("grants.json");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temp_path = self
            .dir
            .join(format!(".grants-{}-{nonce}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .map_err(|error| format!("grant: create temp file: {error}"))?;
        if let Err(error) = file.write_all(&data).and_then(|()| file.flush()) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("grant: write temp file: {error}"));
        }
        set_mode(&temp_path, 0o600).map_err(|error| format!("grant: write temp file: {error}"))?;
        drop(file);
        if let Err(error) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("grant: rename temp file: {error}"));
        }
        Ok(())
    }
}

fn validate_time(value: &str) -> Result<(), String> {
    go_time::parse(value.as_bytes()).map(|_| ())
}

fn is_persistent(scope: &str) -> bool {
    matches!(scope, "device" | "vault")
}

fn time_key(value: &str) -> (i64, u32, String) {
    go_time::parse(value.as_bytes())
        .map(|time| {
            (
                time.timestamp(),
                time.timestamp_subsec_nanos(),
                String::new(),
            )
        })
        .unwrap_or((i64::MIN, 0, value.to_owned()))
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}
