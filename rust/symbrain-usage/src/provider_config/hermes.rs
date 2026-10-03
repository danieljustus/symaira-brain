// Read-only, capability-rooted Hermes credential decoder.
fn nous_auth_path() -> PathBuf {
    env_path("HERMES_HOME", home().join(".hermes")).join("auth.json")
}

fn read_hermes_credentials(path: &Path) -> Option<Vec<u8>> {
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
struct HermesFields(Vec<(String, Box<serde_json::value::RawValue>)>);
impl<'de> Deserialize<'de> for HermesFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = HermesFields;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a credential object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(HermesFields(Vec::new()))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(HermesFields(fields))
            }
        }
        deserializer.deserialize_any(FieldsVisitor)
    }
}

#[derive(Default)]
struct HermesProvider {
    id: String,
    invoke_jwt: String,
    access_token: String,
}

fn decode_hermes_token(contents: &[u8]) -> Option<String> {
    let text = go_json_compatible_text(contents);
    let root: HermesFields = serde_json::from_str(&text).ok()?;
    let mut providers: Vec<HermesProvider> = Vec::new();
    for (name, value) in root.0 {
        if !go_json_field_matches(&name, "providers") {
            continue;
        }
        if value.get() == "null" {
            providers.clear();
            continue;
        }
        let entries: Vec<Box<serde_json::value::RawValue>> =
            serde_json::from_str(value.get()).ok()?;
        let length = entries.len();
        while providers.len() < length {
            providers.push(HermesProvider::default());
        }
        for (index, entry) in entries.into_iter().enumerate() {
            let fields: HermesFields = serde_json::from_str(entry.get()).ok()?;
            for (name, value) in fields.0 {
                let field = if go_json_field_matches(&name, "id") {
                    &mut providers[index].id
                } else if go_json_field_matches(&name, "invoke_jwt") {
                    &mut providers[index].invoke_jwt
                } else if go_json_field_matches(&name, "access_token") {
                    &mut providers[index].access_token
                } else {
                    continue;
                };
                // Go null leaves non-pointer scalar fields unchanged.
                if value.get() != "null" {
                    *field = serde_json::from_str(value.get()).ok()?;
                }
            }
        }
        providers.truncate(length);
    }
    let provider = providers
        .into_iter()
        .find(|provider| provider.id == "nous")?;
    let token = if provider.invoke_jwt.is_empty() {
        provider.access_token
    } else {
        provider.invoke_jwt
    };
    (!token.is_empty()).then_some(token)
}

fn nous_file_token(path: &Path) -> Option<String> {
    let token = decode_hermes_token(&read_hermes_credentials(path)?)?;
    (!token.contains('.') || nous_jwt_is_live(&token)).then_some(token)
}

fn nous_file_requires_go(path: &Path) -> bool {
    let Some(token) =
        read_hermes_credentials(path).and_then(|contents| decode_hermes_token(&contents))
    else {
        return false;
    };
    jwt_expiry(&token).is_some_and(jwt_expiry_out_of_range)
}
