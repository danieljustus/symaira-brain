//! Go-exact decode of the symskills config files `config.toml` and `.symskills.toml`.
//!
//! Go reads those files with `BurntSushi/toml` v1.6.0 in two independent ways:
//!
//! * `config.Load` (corekit `configkit.Loader`) decodes the file into a generic
//!   `map[string]interface{}` and copies entries onto `config.Config` by exact,
//!   case-sensitive `json` tag, in struct order: `library_dir`, `render_dir`,
//!   `cache_dir`, `profiles_dir`, `base_dir`, then `vcs` (see [`load`]);
//! * `config.LoadTargets` / `config.LoadCapabilities` decode the file straight
//!   into a typed struct with case-insensitive field matching (see
//!   [`load_targets`]).
//!
//! The native contract is deliberately asymmetric. A TOML syntax error, invalid
//! UTF-8, a typed-decode type error, or any input whose Go behaviour is not
//! reproduced here yields [`NeedsGo`]: the caller must keep the Go route, which
//! owns the exact message. An input is never guessed: [`LoadOutcome::Loaded`]
//! and [`LoadOutcome::Error`] are only returned when Go demonstrably produces
//! the same value or the same error text (`tests/config_toml_oracle.rs` replays
//! a Go-generated corpus of thousands of documents). The grammar itself comes
//! from `toml_edit`; every document it rejects is classified as
//! [`NeedsGo::Syntax`], so the only unsafe direction (accepting a document Go
//! rejects, or decoding a different value) is what the oracle replay guards.

use std::collections::BTreeMap;

use toml_edit::{Datetime, Document, Item, Table, TableLike, Value};

/// Why a config file cannot be decoded natively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NeedsGo {
    /// The bytes are not valid UTF-8 (Go reports the first bad byte).
    Utf8,
    /// `toml_edit` rejected the document; Go owns the exact syntax message.
    Syntax,
    /// The typed decode hits a type mismatch, whose message embeds the TOML
    /// key path and a source line.
    TypeMismatch,
    /// Two keys map onto one struct field by case folding; Go picks one by
    /// map iteration order.
    FieldCollision,
    /// A string that Go would print through `%q` is not plain printable ASCII.
    Unquotable,
}

/// The `config.Config` fields a config file can set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillsConfig {
    /// `library_dir`.
    pub library_dir: String,
    /// `render_dir`.
    pub render_dir: String,
    /// `cache_dir`.
    pub cache_dir: String,
    /// `profiles_dir`.
    pub profiles_dir: String,
    /// `base_dir`.
    pub base_dir: String,
    /// `Config.VCSEnabled()`: `vcs.enabled` unset or true.
    pub vcs_enabled: bool,
}

/// One config file that exists on disk.
#[derive(Clone, Copy, Debug)]
pub struct ConfigFile<'a> {
    /// The path Go formats into its error messages.
    pub path: &'a str,
    /// The raw file bytes.
    pub content: &'a [u8],
}

/// Result of replaying `config.Load`'s two file merges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadOutcome {
    /// `Load` succeeded with this config (before `SYMSKILLS_*` overrides).
    Loaded(SkillsConfig),
    /// `Load` failed with exactly this error text.
    Error(String),
    /// Go must decide.
    NeedsGo(NeedsGo),
}

/// A datetime Go refuses although `toml_edit` accepts it: `BurntSushi` rejects
/// the leap second `:60` that TOML 1.1 allows.
fn is_go_invalid_datetime(datetime: &Datetime) -> bool {
    datetime
        .time
        .is_some_and(|time| time.second.is_some_and(|second| second > 59))
}

/// Whether the document holds anything `BurntSushi` decodes differently from
/// `toml_edit` although both accept it syntactically:
///
/// * a datetime Go rejects ([`is_go_invalid_datetime`]);
/// * an inline table spanning several lines. TOML 1.1 allows that, but
///   `BurntSushi` only tolerates the newline after `{`, after a comma and before
///   `}`, while `toml_edit` also accepts one between a key and its `=`.
fn has_go_divergent_value(root: &Table, text: &str) -> bool {
    enum Node<'a> {
        Item(&'a Item),
        Value(&'a Value),
    }
    let mut stack: Vec<Node<'_>> = root.iter().map(|(_, item)| Node::Item(item)).collect();
    while let Some(node) = stack.pop() {
        match node {
            Node::Item(Item::Value(value)) | Node::Value(value) => match value {
                Value::Datetime(datetime) => {
                    if is_go_invalid_datetime(datetime.value()) {
                        return true;
                    }
                }
                Value::Array(array) => stack.extend(array.iter().map(Node::Value)),
                Value::InlineTable(table) => {
                    let multiline = table
                        .span()
                        .and_then(|span| text.get(span))
                        .is_none_or(|source| source.contains(['\n', '\r']));
                    if multiline {
                        return true;
                    }
                    stack.extend(table.iter().map(|(_, value)| Node::Value(value)));
                }
                _ => {}
            },
            Node::Item(Item::Table(table)) => {
                stack.extend(table.iter().map(|(_, item)| Node::Item(item)));
            }
            Node::Item(Item::ArrayOfTables(tables)) => {
                for table in tables {
                    stack.extend(table.iter().map(|(_, item)| Node::Item(item)));
                }
            }
            Node::Item(Item::None) => {}
        }
    }
    false
}

/// Decodes `text` the way `toml.DecodeFile` hands it to the parser: valid
/// UTF-8 with at most one leading byte-order mark removed. Documents
/// `toml_edit` accepts but `BurntSushi` rejects are routed to Go here.
fn parse(content: &[u8]) -> Result<Document<&str>, NeedsGo> {
    let text = std::str::from_utf8(content).map_err(|_| NeedsGo::Utf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // A second byte-order mark is a syntax error for Go; `toml_edit` skips it.
    if text.starts_with('\u{feff}') {
        return Err(NeedsGo::Syntax);
    }
    let document = Document::parse(text).map_err(|_| NeedsGo::Syntax)?;
    if has_go_divergent_value(document.as_table(), text) {
        return Err(NeedsGo::Syntax);
    }
    Ok(document)
}

/// Go's dynamic type name for a decoded TOML value, as `%T` prints it.
fn go_type(item: &Item) -> &'static str {
    match item {
        Item::Value(Value::String(_)) => "string",
        Item::Value(Value::Integer(_)) => "int64",
        Item::Value(Value::Float(_)) => "float64",
        Item::Value(Value::Boolean(_)) => "bool",
        Item::Value(Value::Datetime(_)) => "time.Time",
        Item::Value(Value::Array(_)) => "[]interface {}",
        Item::Value(Value::InlineTable(_)) | Item::Table(_) | Item::None => {
            "map[string]interface {}"
        }
        Item::ArrayOfTables(_) => "[]map[string]interface {}",
    }
}

/// `strconv.ParseBool`.
fn parse_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `%q` for text whose Go rendering is the text between plain quotes.
fn quote_plain(text: &str) -> Result<String, NeedsGo> {
    if text
        .bytes()
        .all(|byte| (0x20..0x7f).contains(&byte) && byte != b'"' && byte != b'\\')
    {
        Ok(format!("\"{text}\""))
    } else {
        Err(NeedsGo::Unquotable)
    }
}

/// `applyMapToStruct` over one decoded file. `Ok(Err(message))` is the Go
/// error text after the `failed to apply <path>: ` prefix.
fn apply(config: &mut SkillsConfig, root: &Table) -> Result<Result<(), String>, NeedsGo> {
    let strings: [(&str, &mut String); 5] = [
        ("library_dir", &mut config.library_dir),
        ("render_dir", &mut config.render_dir),
        ("cache_dir", &mut config.cache_dir),
        ("profiles_dir", &mut config.profiles_dir),
        ("base_dir", &mut config.base_dir),
    ];
    for (key, slot) in strings {
        match root.get(key) {
            None => {}
            Some(Item::Value(Value::String(text))) => slot.clone_from(text.value()),
            Some(other) => {
                return Ok(Err(format!(
                    "field \"{key}\": cannot convert {} to string",
                    go_type(other)
                )));
            }
        }
    }
    // `vcs` is a struct field: anything but a table is silently ignored.
    let Some(vcs) = root.get("vcs").and_then(Item::as_table_like) else {
        return Ok(Ok(()));
    };
    match vcs.get("enabled") {
        None => {}
        Some(Item::Value(Value::Boolean(flag))) => config.vcs_enabled = *flag.value(),
        Some(Item::Value(Value::String(text))) => {
            if let Some(flag) = parse_bool(text.value()) {
                config.vcs_enabled = flag;
            } else {
                let quoted = quote_plain(text.value())?;
                return Ok(Err(format!(
                    "field \"vcs\": field \"enabled\": cannot parse {quoted} as bool: \
                     strconv.ParseBool: parsing {quoted}: invalid syntax"
                )));
            }
        }
        Some(other) => {
            return Ok(Err(format!(
                "field \"vcs\": field \"enabled\": cannot convert {} to bool",
                go_type(other)
            )));
        }
    }
    Ok(Ok(()))
}

/// Replays `config.Load` for the global then the project file: each existing
/// file is decoded and applied onto `defaults`; the first failure ends the
/// load with the wrapped error Go returns. `SYMSKILLS_*` overrides and the
/// discard-on-error behaviour of `loadSkillsEnv` stay with the caller.
#[must_use]
pub fn load(
    defaults: &SkillsConfig,
    global: Option<ConfigFile<'_>>,
    project: Option<ConfigFile<'_>>,
) -> LoadOutcome {
    let mut config = defaults.clone();
    for (label, file) in [("global", global), ("project", project)] {
        let Some(file) = file else { continue };
        let document = match parse(file.content) {
            Ok(document) => document,
            Err(reason) => return LoadOutcome::NeedsGo(reason),
        };
        match apply(&mut config, document.as_table()) {
            Err(reason) => return LoadOutcome::NeedsGo(reason),
            Ok(Err(message)) => {
                return LoadOutcome::Error(format!(
                    "{label} config error: failed to apply {}: {message}",
                    file.path
                ));
            }
            Ok(Ok(())) => {}
        }
    }
    LoadOutcome::Loaded(config)
}

/// `config.CustomTarget`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CustomTarget {
    /// `name`.
    pub name: String,
    /// `display_name`.
    pub display_name: String,
    /// `binary_name`.
    pub binary_name: String,
    /// `skill_root_user`.
    pub skill_root_user: String,
    /// `skill_root_project`.
    pub skill_root_project: String,
    /// `metadata_file`.
    pub metadata_file: String,
    /// `metadata_template`.
    pub metadata_template: String,
    /// `overlay_dir`.
    pub overlay_dir: String,
    /// `capabilities` (empty when absent or not a table).
    pub capabilities: BTreeMap<String, bool>,
}

/// What `config.LoadTargets` and `config.LoadCapabilities` return together.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetsConfig {
    /// `LoadTargets`: the last file with a non-empty `[[targets]]` wins.
    pub targets: Vec<CustomTarget>,
    /// `LoadCapabilities`: `None` when no file declared a capabilities target.
    pub capabilities: Option<BTreeMap<String, BTreeMap<String, bool>>>,
}

/// Result of replaying the typed targets/capabilities decode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetsOutcome {
    /// Both loaders succeed with these values.
    Loaded(TargetsConfig),
    /// Go must decide (every failure message embeds a TOML key path).
    NeedsGo(NeedsGo),
}

/// Go's `strings.EqualFold` for a pure-ASCII `field` name: the only non-ASCII
/// characters that simple-fold to ASCII letters are U+017F (to `s`) and U+212A
/// (to `k`).
fn fold_equals(field: &str, key: &str) -> bool {
    let mut left = field.chars();
    let mut right = key.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) => {
                let folded = match b {
                    '\u{17f}' => 's',
                    '\u{212a}' => 'k',
                    other => other.to_ascii_lowercase(),
                };
                if a != folded {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `unifyStruct`'s field lookup: every key maps to the field it equals exactly
/// or, failing that, by case folding. Two keys on one field are ambiguous.
fn match_fields<'a, const N: usize>(
    table: &'a dyn TableLike,
    fields: [&'static str; N],
) -> Result<[Option<&'a Item>; N], NeedsGo> {
    let mut slots: [Option<&'a Item>; N] = [None; N];
    for (key, item) in table.iter() {
        let index = fields
            .iter()
            .position(|field| *field == key)
            .or_else(|| fields.iter().position(|field| fold_equals(field, key)));
        if let Some(index) = index
            && slots[index].replace(item).is_some()
        {
            return Err(NeedsGo::FieldCollision);
        }
    }
    Ok(slots)
}

/// A typed `string` field: any other TOML type is a Go decode error.
fn typed_string(item: Option<&Item>) -> Result<String, NeedsGo> {
    match item {
        None => Ok(String::new()),
        Some(Item::Value(Value::String(text))) => Ok(text.value().clone()),
        Some(_) => Err(NeedsGo::TypeMismatch),
    }
}

/// A typed `map[string]bool`. `BurntSushi` silently ignores a value that is not
/// a table (`unifyMap` returns nil for it); a non-bool entry is an error.
fn bool_map(item: &Item) -> Result<Option<BTreeMap<String, bool>>, NeedsGo> {
    let Some(table) = item.as_table_like() else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for (name, value) in table.iter() {
        match value {
            Item::Value(Value::Boolean(flag)) => {
                out.insert(name.to_owned(), *flag.value());
            }
            _ => return Err(NeedsGo::TypeMismatch),
        }
    }
    Ok(Some(out))
}

fn decode_target(table: &dyn TableLike) -> Result<CustomTarget, NeedsGo> {
    let [
        name,
        display_name,
        binary_name,
        skill_root_user,
        skill_root_project,
        metadata_file,
        metadata_template,
        overlay_dir,
        capabilities,
    ] = match_fields(
        table,
        [
            "name",
            "display_name",
            "binary_name",
            "skill_root_user",
            "skill_root_project",
            "metadata_file",
            "metadata_template",
            "overlay_dir",
            "capabilities",
        ],
    )?;
    let capabilities = match capabilities {
        Some(item) => bool_map(item)?.unwrap_or_default(),
        None => BTreeMap::new(),
    };
    Ok(CustomTarget {
        name: typed_string(name)?,
        display_name: typed_string(display_name)?,
        binary_name: typed_string(binary_name)?,
        skill_root_user: typed_string(skill_root_user)?,
        skill_root_project: typed_string(skill_root_project)?,
        metadata_file: typed_string(metadata_file)?,
        metadata_template: typed_string(metadata_template)?,
        overlay_dir: typed_string(overlay_dir)?,
        capabilities,
    })
}

type CapabilityTables = BTreeMap<String, BTreeMap<String, bool>>;

/// One file decoded into `targetsFile`.
struct TypedFile {
    targets: Vec<CustomTarget>,
    capabilities: Option<CapabilityTables>,
}

fn decode_typed(root: &Table) -> Result<TypedFile, NeedsGo> {
    let [targets, capabilities] = match_fields(root, ["targets", "capabilities"])?;
    let mut decoded = Vec::new();
    match targets {
        None => {}
        Some(Item::ArrayOfTables(tables)) => {
            for table in tables {
                decoded.push(decode_target(table)?);
            }
        }
        Some(Item::Value(Value::Array(array))) => {
            for value in array {
                let Value::InlineTable(table) = value else {
                    return Err(NeedsGo::TypeMismatch);
                };
                decoded.push(decode_target(table)?);
            }
        }
        Some(_) => return Err(NeedsGo::TypeMismatch),
    }
    let capabilities = match capabilities.and_then(Item::as_table_like) {
        None => None,
        Some(table) => {
            let mut out = CapabilityTables::new();
            for (target, value) in table.iter() {
                // A target whose value is not a table still gets an empty map.
                out.insert(target.to_owned(), bool_map(value)?.unwrap_or_default());
            }
            Some(out)
        }
    };
    Ok(TypedFile {
        targets: decoded,
        capabilities,
    })
}

/// Replays `LoadTargets` + `LoadCapabilities` (`mergeTargetsFile` over the
/// global then the project file).
#[must_use]
pub fn load_targets(
    global: Option<ConfigFile<'_>>,
    project: Option<ConfigFile<'_>>,
) -> TargetsOutcome {
    let mut merged = TargetsConfig::default();
    for file in [global, project].into_iter().flatten() {
        let typed = match parse(file.content).and_then(|document| decode_typed(document.as_table()))
        {
            Ok(typed) => typed,
            Err(reason) => return TargetsOutcome::NeedsGo(reason),
        };
        if !typed.targets.is_empty() {
            merged.targets = typed.targets;
        }
        for (target, capabilities) in typed.capabilities.into_iter().flatten() {
            merged
                .capabilities
                .get_or_insert_with(BTreeMap::new)
                .entry(target)
                .or_default()
                .extend(capabilities);
        }
    }
    TargetsOutcome::Loaded(merged)
}
