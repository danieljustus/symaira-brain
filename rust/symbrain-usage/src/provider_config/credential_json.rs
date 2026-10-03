// Bounded read-only credential files and ordered Go-compatible JSON objects.
fn read_provider_credentials(path: &Path) -> Option<Vec<u8>> {
    let parent =
        cap_std::fs::Dir::open_ambient_dir(path.parent()?, cap_std::ambient_authority()).ok()?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = parent.open_with(path.file_name()?, &options).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
        return None;
    }
    let mut contents = Vec::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    (contents.len() as u64 <= MAX_CREDENTIAL_FILE_BYTES).then_some(contents)
}

// RawValue preserves duplicate ordering and ignored JSON numbers without
// decoding unknown metadata to float64. String normalization mirrors Go's
// encoding/json UTF-8 and surrogate behavior before typed field decoding.
struct CredentialFields(Vec<(String, Box<serde_json::value::RawValue>)>);
impl<'de> Deserialize<'de> for CredentialFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = CredentialFields;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a credential object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(CredentialFields(Vec::new()))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(CredentialFields(fields))
            }
        }
        deserializer.deserialize_any(FieldsVisitor)
    }
}
