//! Cobra/pflag-compatible command resolution driven by the frozen Go command
//! tree (`testdata/port/cli/tree.json`).
//!
//! The tree carries, per command, the exact `--help` bytes, usage line, flags
//! and sub-commands captured from the Go binary. This module re-implements the
//! parts of `cobra.Command.Find`/`ExecuteC` and `pflag.FlagSet.Parse` that
//! decide which command runs, which flags it received and which error text is
//! produced, byte for byte.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

const TREE_JSON: &str = include_str!("../../../testdata/port/cli/tree.json");

/// Commands present in the Go tree that the Rust CLI retired on purpose.
const RETIRED: &[&str] = &["compat-sidecar"];

#[derive(Debug, Deserialize)]
struct RawTree {
    nodes: Vec<RawNode>,
}

#[derive(Debug, Deserialize)]
struct RawNode {
    path: Vec<String>,
    help: String,
    aliases: Vec<String>,
    flags: Vec<RawFlag>,
    global_flags: Vec<RawFlag>,
    children: Vec<RawChild>,
}

#[derive(Clone, Debug, Deserialize)]
struct RawFlag {
    short: Option<String>,
    name: String,
    #[serde(rename = "type")]
    ty: String,
    default: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawChild {
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlagType {
    Bool,
    String,
    Int,
    Float,
    StringArray,
}

#[derive(Clone, Debug)]
pub struct Flag {
    pub name: String,
    pub short: Option<u8>,
    pub ty: FlagType,
    pub default: Option<String>,
}

#[derive(Debug)]
pub struct Node {
    pub path: Vec<String>,
    pub help: String,
    pub aliases: Vec<String>,
    pub children: Vec<usize>,
    pub flags: Vec<Flag>,
    pub inherited: Vec<Flag>,
    pub use_name: String,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    by_path: HashMap<Vec<String>, usize>,
}

fn convert(raw: &RawFlag) -> Flag {
    let ty = match raw.ty.as_str() {
        "bool" => FlagType::Bool,
        "int" => FlagType::Int,
        "float" => FlagType::Float,
        "stringArray" => FlagType::StringArray,
        _ => FlagType::String,
    };
    Flag {
        name: raw.name.clone(),
        short: raw.short.as_ref().and_then(|s| s.bytes().next()),
        ty,
        default: raw.default.clone(),
    }
}

impl Tree {
    /// Loads the embedded tree and removes retired commands.
    pub fn load() -> Self {
        let raw: RawTree = serde_json::from_str(TREE_JSON).expect("embedded CLI tree is valid");
        let retired = |path: &[String]| path.first().is_some_and(|c| RETIRED.contains(&c.as_str()));
        let mut nodes: Vec<Node> = Vec::new();
        let mut by_path = HashMap::new();
        for node in raw.nodes.iter().filter(|n| !retired(&n.path)) {
            let mut help = node.help.clone();
            if node.path.is_empty() {
                help = remove_retired_listing(&help);
            }
            let mut flags: Vec<Flag> = node.flags.iter().map(convert).collect();
            if node.path.as_slice() == ["daemon"] {
                // `daemon --mcp-mode` is hidden from help but accepted.
                flags.push(Flag {
                    name: "mcp-mode".into(),
                    short: None,
                    ty: FlagType::Bool,
                    default: None,
                });
            }
            let use_name = node
                .path
                .last()
                .cloned()
                .unwrap_or_else(|| "symbrowse".to_owned());
            by_path.insert(node.path.clone(), nodes.len());
            nodes.push(Node {
                path: node.path.clone(),
                help,
                aliases: node.aliases.clone(),
                children: Vec::new(),
                flags,
                inherited: node.global_flags.iter().map(convert).collect(),
                use_name,
            });
        }
        for raw_node in raw.nodes.iter().filter(|n| !retired(&n.path)) {
            let parent = by_path[&raw_node.path];
            let mut children: Vec<usize> = raw_node
                .children
                .iter()
                .filter_map(|child| {
                    let mut path = raw_node.path.clone();
                    path.push(child.name.clone());
                    by_path.get(&path).copied()
                })
                .collect();
            children.sort_by(|a, b| nodes[*a].use_name.cmp(&nodes[*b].use_name));
            nodes[parent].children = children;
        }
        Self { nodes, by_path }
    }

    pub fn node(&self, path: &[String]) -> Option<&Node> {
        self.by_path.get(path).map(|i| &self.nodes[*i])
    }

    pub fn index(&self, path: &[String]) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    /// All flags addressable on a node (own first, then inherited).
    pub fn flagset(&self, index: usize) -> Vec<&Flag> {
        let node = &self.nodes[index];
        let mut set: Vec<&Flag> = node.flags.iter().collect();
        for flag in &node.inherited {
            if !set.iter().any(|f| f.name == flag.name) {
                set.push(flag);
            }
        }
        set
    }

    fn find_next(&self, index: usize, name: &str) -> Option<usize> {
        self.nodes[index].children.iter().copied().find(|c| {
            self.nodes[*c].use_name == name || self.nodes[*c].aliases.iter().any(|a| a == name)
        })
    }

    /// Usage text only (the help template without the description).
    pub fn usage(&self, index: usize) -> String {
        let help = &self.nodes[index].help;
        let usage = help
            .find("Usage:\n")
            .map_or_else(|| help.clone(), |at| help[at..].to_owned());
        // `Usage()` runs before cobra registers the implicit help/version flags.
        usage
            .split_inclusive('\n')
            .filter(|line| {
                !line.starts_with("  -h, --help ") && !line.starts_with("  -v, --version ")
            })
            .collect()
    }

    fn command_path(&self, index: usize) -> String {
        let mut parts = vec!["symbrowse".to_owned()];
        parts.extend(self.nodes[index].path.iter().cloned());
        parts.join(" ")
    }
}

fn remove_retired_listing(help: &str) -> String {
    help.lines()
        .filter(|line| {
            !RETIRED
                .iter()
                .any(|name| line.starts_with(&format!("  {name} ")))
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

// ---------------------------------------------------------------------------
// Find

fn has_no_opt_def_val(name: &str, flags: &[&Flag]) -> bool {
    flags
        .iter()
        .find(|f| f.name == name)
        .is_some_and(|f| f.ty == FlagType::Bool)
}

fn short_has_no_opt_def_val(name: &str, flags: &[&Flag]) -> bool {
    let Some(first) = name.bytes().next() else {
        return false;
    };
    flags
        .iter()
        .find(|f| f.short == Some(first))
        .is_some_and(|f| f.ty == FlagType::Bool)
}

/// The `case` guards shared by cobra's `stripFlags` and `argsMinusFirstX`:
/// a flag token that is followed by its value as a separate argument.
fn takes_separate_value(s: &str, flags: &[&Flag]) -> bool {
    let long = s.starts_with("--") && !s.contains('=') && !has_no_opt_def_val(&s[2..], flags);
    let short = s.starts_with('-')
        && !s.contains('=')
        && s.len() == 2
        && !short_has_no_opt_def_val(&s[1..], flags);
    long || short
}

fn strip_flags(args: &[String], flags: &[&Flag]) -> Vec<String> {
    let mut commands = Vec::new();
    let mut rest = args;
    while let Some((s, tail)) = rest.split_first() {
        rest = tail;
        if s == "--" {
            break;
        }
        if takes_separate_value(s, flags) {
            if rest.len() <= 1 {
                break;
            }
            rest = &rest[1..];
        } else if !s.is_empty() && !s.starts_with('-') {
            commands.push(s.clone());
        }
    }
    commands
}

fn args_minus_first(args: &[String], x: &str, flags: &[&Flag]) -> Vec<String> {
    let mut pos = 0;
    while pos < args.len() {
        let s = &args[pos];
        if s == "--" {
            break;
        }
        if takes_separate_value(s, flags) {
            pos += 2;
            continue;
        }
        if !s.starts_with('-') && s == x {
            let mut out = args[..pos].to_vec();
            out.extend_from_slice(&args[pos + 1..]);
            return out;
        }
        pos += 1;
    }
    args.to_vec()
}

#[derive(Debug)]
pub struct Found {
    pub index: usize,
    pub args: Vec<String>,
    pub legacy_error: Option<String>,
}

pub fn find(tree: &Tree, args: &[String]) -> Found {
    let mut index = 0;
    let mut current: Vec<String> = args.to_vec();
    loop {
        let flags = tree.flagset(index);
        let words = strip_flags(&current, &flags);
        let Some(next) = words.first() else { break };
        let Some(child) = tree.find_next(index, next) else {
            break;
        };
        current = args_minus_first(&current, next, &flags);
        index = child;
    }
    let legacy_error = legacy_args(tree, index, &current);
    Found {
        index,
        args: current,
        legacy_error,
    }
}

fn legacy_args(tree: &Tree, index: usize, args: &[String]) -> Option<String> {
    if !matches!(args_validator(&tree.nodes[index].path), ArgSpec::Legacy) {
        return None;
    }
    let node = &tree.nodes[index];
    if node.children.is_empty() {
        return None;
    }
    let flags = tree.flagset(index);
    let words = strip_flags(args, &flags);
    if !node.path.is_empty() || words.is_empty() {
        return None;
    }
    Some(format!(
        "unknown command {} for {}{}",
        go_quote(&words[0]),
        go_quote(&tree.command_path(index)),
        suggestions(tree, index, &words[0])
    ))
}

fn go_quote(value: &str) -> String {
    symbrowse_core::go_quote::quote(value)
}

fn suggestions(tree: &Tree, index: usize, typed: &str) -> String {
    let mut found = Vec::new();
    // The help and completion commands are real children at execution time.
    let mut names: Vec<String> = tree.nodes[index]
        .children
        .iter()
        .map(|c| tree.nodes[*c].use_name.clone())
        .collect();
    names.sort();
    for name in names {
        let distance = levenshtein(&typed.to_lowercase(), &name.to_lowercase());
        if distance <= 2 || name.to_lowercase().starts_with(&typed.to_lowercase()) {
            found.push(name);
        }
    }
    if found.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nDid you mean this?\n");
    for name in found {
        out.push('\t');
        out.push_str(&name);
        out.push('\n');
    }
    out
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            row.push(
                (previous[j] + cost)
                    .min(previous[j + 1] + 1)
                    .min(row[j] + 1),
            );
        }
        previous = row;
    }
    previous[b.len()]
}

// ---------------------------------------------------------------------------
// Argument validators

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgSpec {
    /// `Args == nil`: cobra's legacy behaviour.
    Legacy,
    No,
    Arbitrary,
    Min(usize),
    Max(usize),
    Exact(usize),
    Range(usize, usize),
    /// Closure validators with command specific text.
    Custom,
}

/// `Args:` of every Go command, keyed by command path.
pub fn args_validator(path: &[String]) -> ArgSpec {
    let key = path.join(" ");
    match key.as_str() {
        "batch" | "eval" => ArgSpec::Arbitrary,
        "open" | "goto" | "back" | "forward" | "reload" | "wait" | "read" | "screenshot"
        | "a11y" | "tab new" | "tab close" | "dialog accept" | "network unroute"
        | "set offline" => ArgSpec::Max(1),
        "check" | "click" | "dblclick" | "fill" | "focus" | "hover" | "press" | "scroll"
        | "scrollintoview" | "select" | "type" | "uncheck" | "find" | "cookies set"
        | "set device" | "get text" | "get html" | "get value" | "get attr" | "get title"
        | "get url" | "get count" | "get box" | "get styles" | "is visible" | "is enabled"
        | "is checked" => ArgSpec::Custom,
        "snapshot" | "downloads" | "version" | "doctor" | "mcp" | "upgrade" | "profiles"
        | "handoff" | "watch" | "daemon" | "tab list" | "tab window window" | "frame tree"
        | "frame main" | "dialog status" | "dialog dismiss" | "cookies list" | "journal tail"
        | "journal show" | "oob status" | "session list" | "session info" | "session id"
        | "state list" | "state clean" | "state key init" | "cache list" | "cache clear"
        | "config show" | "console list" | "console clear" | "errors list" | "errors clear"
        | "diff snapshot" | "diff screenshot" | "daemon status" | "daemon stop"
        | "network requests" | "trace export" | "flow list" | "state" | "state key" | "cache"
        | "errors" | "console" | "network" | "session" | "journal" | "oob" | "auth" | "cookies"
        | "storage" | "set" | "policy" | "trace" => ArgSpec::No,
        "upload" => ArgSpec::Min(2),
        "tab switch" | "frame select" | "dialog auto" | "auth login" | "cookies clear"
        | "policy explain" | "trace replay" | "state save" | "state load" | "state show"
        | "state clear" | "storage clear" | "cache get" | "network request" | "network route"
        | "network har" | "set headers" | "set media" | "set user-agent" | "flow validate"
        | "flow run" | "flow record" => ArgSpec::Exact(1),
        "set geo" | "diff url" => ArgSpec::Exact(2),
        "storage set" => ArgSpec::Exact(3),
        "set viewport" => ArgSpec::Range(2, 3),
        "storage get" => ArgSpec::Range(1, 2),
        _ => ArgSpec::Legacy,
    }
}

/// Whether the Go command defines `Run`/`RunE`.
pub fn runnable(tree: &Tree, index: usize) -> bool {
    let node = &tree.nodes[index];
    if node.children.is_empty() {
        return true;
    }
    matches!(
        node.path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
        ["daemon"]
    )
}

// ---------------------------------------------------------------------------
// Flag parsing

#[derive(Clone, Debug, Default)]
pub struct FlagValues {
    values: BTreeMap<String, Vec<String>>,
    changed: Vec<String>,
}

impl FlagValues {
    pub fn changed(&self, name: &str) -> bool {
        self.changed.iter().any(|n| n == name)
    }

    pub fn string(&self, name: &str, default: &str) -> String {
        self.values
            .get(name)
            .and_then(|v| v.last().cloned())
            .unwrap_or_else(|| default.to_owned())
    }

    pub fn bool(&self, name: &str) -> bool {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .is_some_and(|v| parse_bool(v) == Some(true))
    }

    pub fn int(&self, name: &str, default: i64) -> i64 {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .and_then(|v| parse_int(v).ok())
            .unwrap_or(default)
    }

    pub fn float(&self, name: &str, default: f64) -> f64 {
        self.values
            .get(name)
            .and_then(|v| v.last())
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn array(&self, name: &str) -> Vec<String> {
        self.values.get(name).cloned().unwrap_or_default()
    }
}

pub fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `strconv.ParseInt(s, 0, 64)`.
pub fn parse_int(value: &str) -> Result<i64, &'static str> {
    let (negative, body) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    if body.is_empty() {
        return Err("invalid syntax");
    }
    let (base, digits, prefixed) =
        if let Some(rest) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            (16, rest, true)
        } else if let Some(rest) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
            (2, rest, true)
        } else if let Some(rest) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
            (8, rest, true)
        } else if body.len() > 1 && body.starts_with('0') {
            (8, &body[1..], true)
        } else {
            (10, body, false)
        };
    let clean = underscore_ok(digits, prefixed).ok_or("invalid syntax")?;
    if clean.is_empty() {
        return Err("invalid syntax");
    }
    let magnitude = u128::from_str_radix(&clean, base).map_err(|e| {
        if matches!(e.kind(), std::num::IntErrorKind::PosOverflow) {
            "value out of range"
        } else {
            "invalid syntax"
        }
    })?;
    if negative {
        if magnitude > 1u128 << 63 {
            return Err("value out of range");
        }
        Ok((magnitude as i128).wrapping_neg() as i64)
    } else if magnitude > i64::MAX as u128 {
        Err("value out of range")
    } else {
        Ok(magnitude as i64)
    }
}

fn underscore_ok(digits: &str, prefixed: bool) -> Option<String> {
    if !digits.contains('_') {
        return Some(digits.to_owned());
    }
    if !prefixed
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_')
        && !digits.starts_with('_')
    {
        // base-0 allows underscores only with a base prefix or between digits
        // of a prefixed literal; plain decimal underscores are a syntax error.
        return None;
    }
    if digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    Some(digits.replace('_', ""))
}

fn parse_float(value: &str) -> Result<(), &'static str> {
    let lower = value.to_ascii_lowercase();
    let trimmed = lower.trim_start_matches(['+', '-']);
    if matches!(trimmed, "inf" | "infinity" | "nan") {
        return Ok(());
    }
    if value.contains('_') || value.is_empty() || value.trim() != value {
        return Err("invalid syntax");
    }
    match value.parse::<f64>() {
        Ok(v) if v.is_infinite() => Err("value out of range"),
        Ok(_) => Ok(()),
        Err(_) => Err("invalid syntax"),
    }
}

fn go_error(fn_name: &str, value: &str, reason: &str) -> String {
    format!("strconv.{fn_name}: parsing {}: {reason}", go_quote(value))
}

fn set_flag(flag: &Flag, value: &str, state: &mut FlagValues) -> Result<(), String> {
    let invalid = |detail: String| {
        let display = match flag.short {
            Some(short) => format!("-{}, --{}", short as char, flag.name),
            None => format!("--{}", flag.name),
        };
        format!(
            "invalid argument {} for {} flag: {detail}",
            go_quote(value),
            go_quote(&display)
        )
    };
    match flag.ty {
        FlagType::Bool => {
            if parse_bool(value).is_none() {
                return Err(invalid(go_error("ParseBool", value, "invalid syntax")));
            }
        }
        FlagType::Int => {
            if let Err(reason) = parse_int(value) {
                return Err(invalid(go_error("ParseInt", value, reason)));
            }
        }
        FlagType::Float => {
            if let Err(reason) = parse_float(value) {
                return Err(invalid(go_error("ParseFloat", value, reason)));
            }
        }
        FlagType::String | FlagType::StringArray => {}
    }
    let entry = state.values.entry(flag.name.clone()).or_default();
    if flag.ty == FlagType::StringArray {
        entry.push(value.to_owned());
    } else {
        entry.clear();
        entry.push(value.to_owned());
    }
    if !state.changed.contains(&flag.name) {
        state.changed.push(flag.name.clone());
    }
    Ok(())
}

pub struct ParsedFlags {
    pub values: FlagValues,
    pub args: Vec<String>,
}

fn go_rune_quote(byte: u8) -> String {
    let c = byte as char;
    match c {
        '\'' => "'\\''".to_owned(),
        '\\' => "'\\\\'".to_owned(),
        c if c.is_ascii_graphic() || c == ' ' => format!("'{c}'"),
        c if (c as u32) >= 0xa1 => format!("'{c}'"),
        _ => format!("'\\x{byte:02x}'"),
    }
}

/// `pflag.FlagSet.Parse` with interspersed arguments.
pub fn parse_flags(flags: &[&Flag], args: &[String]) -> Result<ParsedFlags, ParseFailure> {
    let mut state = FlagValues::default();
    let mut positional = Vec::new();
    let mut rest = args;
    while let Some((s, tail)) = rest.split_first() {
        rest = tail;
        if s.len() <= 1 || !s.starts_with('-') {
            positional.push(s.clone());
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            if long.is_empty() {
                positional.extend(rest.iter().cloned());
                break;
            }
            if long.starts_with(['-', '=', '#']) {
                return Err(ParseFailure {
                    message: format!("bad flag syntax: {s}"),
                    state,
                });
            }
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (long, None),
            };
            let Some(flag) = flags.iter().find(|f| f.name == name) else {
                return Err(ParseFailure {
                    message: format!("unknown flag: --{name}"),
                    state,
                });
            };
            let value = match inline {
                Some(value) => value,
                None if flag.ty == FlagType::Bool => "true".to_owned(),
                None => match rest.split_first() {
                    Some((value, tail)) => {
                        rest = tail;
                        value.clone()
                    }
                    None => {
                        return Err(ParseFailure {
                            message: format!("flag needs an argument: {s}"),
                            state,
                        });
                    }
                },
            };
            if name == "help" && flag.ty == FlagType::Bool {
                // pflag reports -h/--help through the regular value path.
            }
            set_flag(flag, &value, &mut state).map_err(|message| ParseFailure {
                message,
                state: state.clone(),
            })?;
            continue;
        }
        let mut shorts: &str = &s[1..];
        while !shorts.is_empty() {
            let bytes = shorts.as_bytes();
            let c = bytes[0];
            let Some(flag) = flags.iter().find(|f| f.short == Some(c)) else {
                return Err(ParseFailure {
                    message: format!("unknown shorthand flag: {} in -{shorts}", go_rune_quote(c)),
                    state,
                });
            };
            let after = &shorts[1.min(shorts.len())..];
            let value;
            if bytes.len() > 2 && bytes[1] == b'=' {
                value = shorts[2..].to_owned();
                shorts = "";
            } else if flag.ty == FlagType::Bool {
                value = "true".to_owned();
                shorts = after;
            } else if bytes.len() > 1 {
                value = after.to_owned();
                shorts = "";
            } else if let Some((v, tail)) = rest.split_first() {
                value = v.clone();
                rest = tail;
                shorts = "";
            } else {
                return Err(ParseFailure {
                    message: format!("flag needs an argument: {} in -{shorts}", go_rune_quote(c)),
                    state,
                });
            }
            set_flag(flag, &value, &mut state).map_err(|message| ParseFailure {
                message,
                state: state.clone(),
            })?;
        }
    }
    Ok(ParsedFlags {
        values: state,
        args: positional,
    })
}

#[derive(Debug)]
pub struct ParseFailure {
    pub message: String,
    pub state: FlagValues,
}

/// Applies a validator to the positional arguments.
pub fn validate_args(
    spec: ArgSpec,
    tree: &Tree,
    index: usize,
    args: &[String],
) -> Result<(), String> {
    let n = args.len();
    match spec {
        ArgSpec::Legacy | ArgSpec::Arbitrary | ArgSpec::Custom => Ok(()),
        ArgSpec::No if n > 0 => Err(format!(
            "unknown command {} for {}",
            go_quote(&args[0]),
            go_quote(&tree.command_path(index))
        )),
        ArgSpec::Min(min) if n < min => {
            Err(format!("requires at least {min} arg(s), only received {n}"))
        }
        ArgSpec::Max(max) if n > max => Err(format!("accepts at most {max} arg(s), received {n}")),
        ArgSpec::Exact(exact) if n != exact => Err(format!("accepts {exact} arg(s), received {n}")),
        ArgSpec::Range(min, max) if n < min || n > max => Err(format!(
            "accepts between {min} and {max} arg(s), received {n}"
        )),
        _ => Ok(()),
    }
}

pub fn exact_args_error(expected: usize, got: usize) -> String {
    format!("accepts {expected} arg(s), received {got}")
}

pub fn no_args_error(tree: &Tree, index: usize, first: &str) -> String {
    format!(
        "unknown command {} for {}",
        go_quote(first),
        go_quote(&tree.command_path(index))
    )
}
