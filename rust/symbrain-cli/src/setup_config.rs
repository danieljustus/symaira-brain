//! Go-compatible configuration loading for release setup and doctor repair.
//!
//! This mirrors `internal/config.Load` on top of corekit's `configkit`
//! (v0.18.2): the global file, the working directory's `.symbrain.toml`, then
//! `SYMBRAIN_*` overrides, each applied in struct-field order and each value
//! converted the way `configkit.setFieldValue` does. The first failure aborts
//! with the exact Go error text.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use symbrain_core::config::{format_go_quoted_bytes, os_bytes};
use toml_edit::{DocumentMut, Item, TableLike, Value};

#[path = "setup_config_toml.rs"]
mod toml_syntax;

/// Complete Go error text of a failed `config.Load()`.
#[derive(Debug)]
pub(crate) struct ConfigError(Vec<u8>);

impl AsRef<[u8]> for ConfigError {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&String::from_utf8_lossy(&self.0))
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Int,
    Bool,
}

struct Leaf {
    path: &'static [&'static str],
    kind: Kind,
}

const fn leaf(path: &'static [&'static str], kind: Kind) -> Leaf {
    Leaf { path, kind }
}

/// Every `fileConfig` leaf in Go struct-field order.
const LEAVES: [Leaf; 13] = [
    leaf(&["default_profile"], Kind::Text),
    leaf(&["audit", "enabled"], Kind::Bool),
    leaf(&["audit", "verbose"], Kind::Bool),
    leaf(&["gateway", "identity_injection"], Kind::Bool),
    leaf(&["updatecheck", "enabled"], Kind::Bool),
    leaf(&["servers", "vault", "binary_path"], Kind::Text),
    leaf(&["servers", "operate", "binary_path"], Kind::Text),
    leaf(&["servers", "scope", "binary_path"], Kind::Text),
    leaf(&["patterns", "enabled"], Kind::Bool),
    leaf(&["patterns", "promotion_threshold"], Kind::Int),
    leaf(&["modules", "browse"], Kind::Bool),
    leaf(&["modules", "operate"], Kind::Bool),
    leaf(&["modules", "scope"], Kind::Bool),
];

const MODULES: [(&str, &str); 3] = [
    ("browse", "symbrowse"),
    ("operate", "symoperate"),
    ("scope", "symscope"),
];

/// Decoded leaf; only booleans are ever consumed after validation.
enum Converted {
    Bool(bool),
    Other,
}

/// Runs the full Go `config.Load()` and returns the enabled optional cores.
pub(crate) fn enabled_cores() -> Result<BTreeMap<String, bool>, ConfigError> {
    let mut modules = [false; 3];
    let global = symbrain_core::xdg::config_path();
    let wrap = |layer: &str, inner: Vec<u8>| {
        let mut message = b"config: failed to load ".to_vec();
        message.extend_from_slice(&os_bytes(global.as_os_str()));
        message.extend_from_slice(b": ");
        message.extend_from_slice(layer.as_bytes());
        message.extend_from_slice(&inner);
        ConfigError(message)
    };
    merge_file(&mut modules, &global).map_err(|inner| wrap("global config error: ", inner))?;
    // configkit skips the project layer when the working directory is unknown.
    if let Some(directory) = go_getwd() {
        merge_file(&mut modules, &directory.join(".symbrain.toml"))
            .map_err(|inner| wrap("project config error: ", inner))?;
    }
    apply_environment(&mut modules).map_err(|inner| wrap("env override error: ", inner))?;
    Ok(MODULES
        .iter()
        .zip(modules)
        .filter(|(_, enabled)| *enabled)
        .map(|((_, core), _)| ((*core).to_owned(), true))
        .collect())
}

/// Go `os.Getwd`: a valid `$PWD` wins over the kernel's physical path.
fn go_getwd() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Some(pwd) =
            std::env::var_os("PWD").filter(|pwd| pwd.as_encoded_bytes().first() == Some(&b'/'))
            && let (Ok(dot), Ok(candidate)) = (std::fs::metadata("."), std::fs::metadata(&pwd))
            && dot.dev() == candidate.dev()
            && dot.ino() == candidate.ino()
        {
            return Some(PathBuf::from(pwd));
        }
    }
    std::env::current_dir().ok()
}

fn file_error(shown: &[u8], verb: &str, detail: &str) -> Vec<u8> {
    let mut message = format!("{verb} ").into_bytes();
    message.extend_from_slice(shown);
    message.extend_from_slice(b": ");
    message.extend_from_slice(detail.as_bytes());
    message
}

fn merge_file(modules: &mut [bool; 3], path: &Path) -> Result<(), Vec<u8>> {
    // `os.IsNotExist` is ENOENT only; every other stat error falls through to
    // the read, which then reports its own cause.
    if let Err(error) = std::fs::metadata(path)
        && error.kind() == std::io::ErrorKind::NotFound
    {
        return Ok(());
    }
    let shown = os_bytes(path.as_os_str()).into_owned();
    let parse_failure = |detail: &str| file_error(&shown, "failed to parse", detail);
    let mut file = std::fs::File::open(path).map_err(|error| {
        parse_failure(&format!(
            "open {}: {}",
            String::from_utf8_lossy(&shown),
            symbrain_managed::format_io_error(&error)
        ))
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        parse_failure(&format!(
            "read {}: {}",
            String::from_utf8_lossy(&shown),
            symbrain_managed::format_io_error(&error)
        ))
    })?;
    let document = toml_syntax::parse(&bytes).map_err(|detail| parse_failure(&detail))?;
    apply_document(modules, &document)
        .map_err(|detail| file_error(&shown, "failed to apply", &detail))
}

fn apply_document(modules: &mut [bool; 3], document: &DocumentMut) -> Result<(), String> {
    for leaf in &LEAVES {
        let Some(item) = lookup(document, leaf.path) else {
            continue;
        };
        let converted = convert_item(leaf.kind, item).map_err(|cause| {
            let mut message = String::new();
            for segment in leaf.path {
                message.push_str("field ");
                message.push_str(&format_go_quoted_bytes(segment.as_bytes()));
                message.push_str(": ");
            }
            message.push_str(&cause);
            message
        })?;
        record(modules, leaf.path, &converted);
    }
    Ok(())
}

fn record(modules: &mut [bool; 3], path: &[&str], converted: &Converted) {
    if let (["modules", name], Converted::Bool(value)) = (path, converted)
        && let Some(index) = MODULES.iter().position(|(module, _)| module == name)
    {
        modules[index] = *value;
    }
}

fn lookup<'a>(document: &'a DocumentMut, path: &[&str]) -> Option<&'a Item> {
    let mut table: &dyn TableLike = document.as_table();
    for (index, segment) in path.iter().enumerate() {
        let item = table.get(segment)?;
        if index + 1 == path.len() {
            return Some(item);
        }
        // configkit only descends into maps; any other value is ignored.
        table = item.as_table_like()?;
    }
    None
}

/// Go dynamic type of a decoded TOML value (`%T`).
fn go_type(item: &Item) -> &'static str {
    match item {
        Item::Value(Value::String(_)) => "string",
        Item::Value(Value::Integer(_)) => "int64",
        Item::Value(Value::Float(_)) => "float64",
        Item::Value(Value::Boolean(_)) => "bool",
        Item::Value(Value::Datetime(_)) => "time.Time",
        Item::Value(Value::Array(_)) => "[]interface {}",
        Item::ArrayOfTables(_) => "[]map[string]interface {}",
        _ => "map[string]interface {}",
    }
}

fn convert_item(kind: Kind, item: &Item) -> Result<Converted, String> {
    match (kind, item) {
        (Kind::Text, Item::Value(Value::String(_)))
        | (Kind::Int, Item::Value(Value::Integer(_) | Value::Float(_))) => Ok(Converted::Other),
        (Kind::Text, other) => Err(format!("cannot convert {} to string", go_type(other))),
        (Kind::Int, Item::Value(Value::String(text))) => {
            parse_int(text.value().as_bytes()).map(|()| Converted::Other)
        }
        (Kind::Int, other) => Err(format!("cannot convert {} to int", go_type(other))),
        (Kind::Bool, Item::Value(Value::Boolean(value))) => Ok(Converted::Bool(*value.value())),
        (Kind::Bool, Item::Value(Value::String(text))) => {
            parse_bool(text.value().as_bytes()).map(Converted::Bool)
        }
        (Kind::Bool, other) => Err(format!("cannot convert {} to bool", go_type(other))),
    }
}

fn apply_environment(modules: &mut [bool; 3]) -> Result<(), Vec<u8>> {
    for leaf in &LEAVES {
        let variable = format!("SYMBRAIN_{}", leaf.path.join("_").to_uppercase());
        // configkit uses `os.LookupEnv`: a present but empty value is converted
        // (and rejected for bool/int fields) rather than ignored.
        let Some(value) = std::env::var_os(&variable) else {
            continue;
        };
        let bytes = os_bytes(&value);
        let converted = match leaf.kind {
            Kind::Text => Ok(Converted::Other),
            Kind::Int => parse_int(&bytes).map(|()| Converted::Other),
            Kind::Bool => parse_bool(&bytes).map(Converted::Bool),
        }
        .map_err(|cause| format!("env {variable}: {cause}").into_bytes())?;
        record(modules, leaf.path, &converted);
    }
    Ok(())
}

/// Go `strconv.ParseBool` spellings.
pub(crate) fn parse_go_bool(value: &str) -> Result<bool, ()> {
    parse_bool(value.as_bytes()).map_err(|_| ())
}

/// `strconv.ParseBool` with configkit's wrapper text.
fn parse_bool(value: &[u8]) -> Result<bool, String> {
    match value {
        b"1" | b"t" | b"T" | b"TRUE" | b"true" | b"True" => Ok(true),
        b"0" | b"f" | b"F" | b"FALSE" | b"false" | b"False" => Ok(false),
        _ => {
            let quoted = format_go_quoted_bytes(value);
            Err(format!(
                "cannot parse {quoted} as bool: strconv.ParseBool: parsing {quoted}: invalid syntax"
            ))
        }
    }
}

/// `strconv.ParseInt(s, 10, 64)` with configkit's wrapper text.
fn parse_int(value: &[u8]) -> Result<(), String> {
    let digits = match value.first() {
        Some(b'+' | b'-') => &value[1..],
        _ => value,
    };
    let reason = if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        "invalid syntax"
    } else if std::str::from_utf8(value).is_ok_and(|text| text.parse::<i64>().is_ok()) {
        return Ok(());
    } else {
        "value out of range"
    };
    let quoted = format_go_quoted_bytes(value);
    Err(format!(
        "cannot parse {quoted} as int: strconv.ParseInt: parsing {quoted}: {reason}"
    ))
}

#[cfg(test)]
#[path = "setup_config_tests.rs"]
mod tests;
