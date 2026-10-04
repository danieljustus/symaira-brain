//! Prepared public-constructor peer. Runtime and error-byte acceptance pending.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, FixedOffset};
use serde::Deserialize;
use serde_json::{Value, json};
use symbrain_memory::importer::{
    AiderImporter, ApplicationPolicy, CodexMemoryImporter, CuratedMemoryImporter, Fact, Metadata,
    ObsidianImporter, SessionImporter, SessionRef, ShellHistoryImporter,
};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Case {
    id: String,
    family: String,
    root_hex: String,
    #[serde(default)]
    path_hex: String,
    #[serde(default)]
    since: String,
    #[serde(default)]
    allowed_hex: Vec<String>,
    #[serde(default)]
    denied_hex: Vec<String>,
    #[serde(default)]
    filters_hex: Vec<String>,
    #[serde(default)]
    tags_hex: Vec<String>,
    #[serde(default)]
    excluded_folders_hex: Vec<String>,
    #[serde(default)]
    excluded_tags_hex: Vec<String>,
    #[serde(default)]
    direct: bool,
}

fn decode(raw: &str) -> Vec<u8> {
    assert_eq!(raw.len() % 2, 0, "odd hex");
    raw.as_bytes()
        .chunks_exact(2)
        .map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap())
        .collect()
}

fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}
fn values(raw: &[String]) -> Vec<Vec<u8>> {
    raw.iter().map(|v| decode(v)).collect()
}

fn path(raw: &str) -> PathBuf {
    let bytes = decode(raw);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes).into()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes)
            .expect("non-Unix raw path proof pending")
            .into()
    }
}

fn path_hex(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hex(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        hex(path
            .to_str()
            .expect("non-Unix raw path proof pending")
            .as_bytes())
    }
}

fn timestamp(time: Option<DateTime<FixedOffset>>) -> String {
    let Some(time) = time else {
        return "0001-01-01T00:00:00Z".into();
    };
    let mut out = time.format("%Y-%m-%dT%H:%M:%S").to_string();
    if time.timestamp_subsec_nanos() != 0 {
        out.push('.');
        out.push_str(format!("{:09}", time.timestamp_subsec_nanos()).trim_end_matches('0'));
    }
    if time.offset().local_minus_utc() == 0 {
        out.push('Z');
    } else {
        out.push_str(&time.format("%:z").to_string());
    }
    out
}

fn metadata(values: &Metadata) -> Vec<[String; 2]> {
    values
        .iter()
        .map(|(key, value)| [hex(key), hex(value)])
        .collect()
}

fn ref_wire(session: &SessionRef) -> Value {
    json!({"tool_hex":hex(&session.tool),"id_hex":hex(&session.session_id),"path_hex":path_hex(&session.path),
        "modified":timestamp(session.modified_at),"metadata_hex":metadata(&session.metadata)})
}

fn fact_wire(fact: &Fact) -> Value {
    let evidence: Option<Vec<Value>> = if fact.evidence.is_empty() {
        None
    } else {
        Some(fact.evidence.iter().map(|ext| {
        json!({"source_id_hex":hex(&ext.source_id),"source_kind_hex":hex(&ext.source_kind),"text_hex":hex(&ext.text),
            "evidence_hex":hex(&ext.evidence_text),"start":ext.start,"end":ext.end,"alignment":ext.alignment})
    }).collect())
    };
    json!({"content_hex":hex(&fact.content),"source_hex":hex(&fact.source),"id_hex":hex(&fact.session_id),
        "timestamp":timestamp(fact.timestamp),"metadata_hex":metadata(&fact.metadata),"evidence":evidence})
}

fn owner(case: &Case) -> Box<dyn SessionImporter> {
    let root = path(&case.root_hex);
    match case.family.as_str() {
        "aider" => Box::new(AiderImporter::new(vec![root])),
        "curated-memory" => Box::new(CuratedMemoryImporter::new(root)),
        "codex-memory" => Box::new(CodexMemoryImporter::new(
            root,
            ApplicationPolicy {
                allowed: values(&case.allowed_hex),
                denied: values(&case.denied_hex),
            },
        )),
        "shell-history" => Box::new(ShellHistoryImporter::new(
            root,
            true,
            values(&case.filters_hex),
        )),
        "obsidian" => Box::new(ObsidianImporter::new(
            root,
            b"unused-folder".to_vec(),
            values(&case.tags_hex),
            values(&case.excluded_folders_hex),
            values(&case.excluded_tags_hex),
        )),
        other => panic!("unsupported family {other}"),
    }
}

fn execute(case: &Case) -> Value {
    let owner = owner(case);
    let since = if case.since.is_empty() {
        None
    } else {
        Some(DateTime::parse_from_rfc3339(&case.since).unwrap())
    };
    let mut batch = owner.discover(since);
    if case.direct {
        batch.rows = Some(vec![SessionRef {
            tool: owner.name().as_bytes().to_vec(),
            session_id: format!("direct:{}", case.id).into_bytes(),
            path: path(&case.path_hex),
            modified_at: None,
            metadata: BTreeMap::new(),
        }]);
        batch.error = None;
    }
    let sessions = batch
        .rows
        .as_ref()
        .map(|rows| rows.iter().map(ref_wire).collect::<Vec<_>>());
    let mut imports: Option<Vec<Value>> = None;
    for session in batch.rows.into_iter().flatten() {
        for repeat in 0..2 {
            let facts = owner.import(&session);
            imports.get_or_insert_with(Vec::new).push(json!({"repeat":repeat,"id_hex":hex(&session.session_id),
                "facts":facts.rows.as_ref().map(|rows| rows.iter().map(fact_wire).collect::<Vec<_>>()),
                "error":facts.error.map_or_else(String::new,|error| error.to_string())}));
        }
    }
    let traits = owner.traits();
    json!({"id":case.id,"family":case.family,"sessions":sessions,"imports":imports,"error":batch.error.map_or_else(String::new,|error|error.to_string()),
        "traits":{"category":traits.category,"privacy":traits.privacy,"pii_guard":traits.pii_guard,"transcript":traits.transcript,
            "staged":traits.staged,"untrusted":traits.untrusted,"incremental":traits.incremental}})
}

fn main() {
    let input = std::env::args_os().nth(1).expect("owned input JSON path");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!(!cases.is_empty(), "zero cases refused");
    for case in &cases {
        println!("{}", execute(case));
    }
}
