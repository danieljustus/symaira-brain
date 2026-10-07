//! State data shapes accept the Go JSON null slice without discarding values.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(serialize_with = "serialize_go_float")]
    pub expires: f64,
    pub size: i64,
    pub http_only: bool,
    pub secure: bool,
    pub session: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub same_site: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OriginState {
    #[serde(default, deserialize_with = "deserialize_go_cookies")]
    pub cookies: Vec<Cookie>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub local_storage: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session_storage: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct State {
    #[serde(default)]
    pub schema_version: u32,
    pub name: String,
    pub saved_at: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key_source: String,
    pub origins: BTreeMap<String, OriginState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Header {
    pub(super) schema_version: u32,
    pub(super) saved_at: String,
    pub(super) expires_at: String,
    #[serde(default)]
    pub(super) key_source: String,
}

pub(crate) struct StateHeader {
    pub saved_at: String,
    pub expires_at: String,
}

fn serialize_go_float<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if value.is_finite()
        && value.fract() == 0.0
        && *value >= i64::MIN as f64
        && *value <= i64::MAX as f64
    {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

fn deserialize_go_cookies<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Cookie>, D::Error> {
    Ok(Option::<Vec<Cookie>>::deserialize(deserializer)?.unwrap_or_default())
}
