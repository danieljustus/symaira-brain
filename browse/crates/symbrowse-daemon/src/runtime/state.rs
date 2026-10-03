//! State inspection uses the existing Go-compatible store and wire schema.
use super::*;

impl DispatchRuntime {
    pub(super) fn state_command(&self, frame: &Frame) -> HandlerResult {
        let store = Store::new(
            self.spec.state_store_dir(),
            time::Duration::days(self.spec.state_expire_days),
            None,
        )
        .map_err(runtime_error)?;
        let args = frame.args.as_ref().and_then(Value::as_object);
        match frame.cmd.as_str() {
            "state.list" => Ok((
                Some(
                    json!({"schema_version":symbrowse_core::state::SCHEMA_VERSION,"states":go_names(store.list().map_err(runtime_error)?)}),
                ),
                Vec::new(),
            )),
            "state.show" => {
                let empty = serde_json::Map::new();
                let name = required_string(args.unwrap_or(&empty), "name")?;
                Ok((
                    Some(
                        serde_json::to_value(store.metadata(name).map_err(runtime_error)?)
                            .map_err(runtime_error)?,
                    ),
                    Vec::new(),
                ))
            }
            "state.clear" => {
                let empty = serde_json::Map::new();
                let name = required_string(args.unwrap_or(&empty), "name")?;
                store.remove(name).map_err(runtime_error)?;
                Ok((Some(json!({"cleared":name})), Vec::new()))
            }
            "state.clean" => {
                let removed = if let Some(days) = args
                    .and_then(|value| value.get("older_than_days"))
                    .and_then(Value::as_i64)
                    .filter(|days| *days > 0)
                {
                    store
                        .clean_older_than_at(
                            time::Duration::days(days),
                            time::OffsetDateTime::now_utc(),
                        )
                        .map_err(runtime_error)?
                } else {
                    store
                        .clean_at(time::OffsetDateTime::now_utc())
                        .map_err(runtime_error)?
                };
                Ok((Some(json!({"removed":go_names(removed)})), Vec::new()))
            }
            _ => unreachable!(),
        }
    }
}

fn go_names(names: Vec<String>) -> Option<Vec<String>> {
    if names.is_empty() { None } else { Some(names) }
}
