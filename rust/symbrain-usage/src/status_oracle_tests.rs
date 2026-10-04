//! All remote constructor/status paths over the production HTTPS builder.
use super::*;
fn file_state(root: &std::path::Path) -> BTreeMap<String, Value> {
    fn visit(path: &std::path::Path, root: &std::path::Path, values: &mut BTreeMap<String, Value>) {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        let bytes = if metadata.is_file() {
            Some(std::fs::read(path).unwrap())
        } else {
            None
        };
        values.insert(path.strip_prefix(root).unwrap().to_string_lossy().into_owned(),json!({"readonly":metadata.permissions().readonly(),"length":metadata.len(),"modified":metadata.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos().to_string(),"sha256":bytes.map(|b|format!("{:x}",Sha256::digest(&b)))}));
        if metadata.is_dir() {
            for item in std::fs::read_dir(path).unwrap() {
                visit(&item.unwrap().path(), root, values);
            }
        }
    }
    let mut values = BTreeMap::new();
    visit(root, root, &mut values);
    values
}
pub(super) fn compare() -> Value {
    let input: Vec<Value> =
        serde_json::from_slice(&std::fs::read(std::env::var("USAGE_STATUS_GO").unwrap()).unwrap())
            .unwrap();
    assert_eq!(input.len(), 84);
    let mut observed = vec![];
    for row in &input {
        // The same disposable constructor homes/metadata from actual frozen Go.
        for name in [
            "ANTHROPIC_ADMIN_KEY",
            "ANTHROPIC_OAUTH_TOKEN",
            "CODEX_ACCESS_TOKEN",
            "COPILOT_ACCESS_TOKEN",
            "CURSOR_COOKIE",
            "KIMI_CODE_API_KEY",
            "KIMI_AUTH_TOKEN",
            "MOONSHOT_API_KEY",
            "NOUS_PORTAL_ACCESS_TOKEN",
            "OPENCODE_COOKIE",
            "OPENROUTER_API_KEY",
            "CODEX_HOME",
            "KIMI_CODE_HOME",
            "HERMES_HOME",
            "KIMI_CODE_BASE_URL",
            "HERMES_PORTAL_BASE_URL",
            "OPENROUTER_API_URL",
            "OPENCODE_WORKSPACE_ID",
            "USAGE_STATUS_ABSENT",
        ] {
            set(name, "");
        }
        set("HOME", row["home"].as_str().unwrap());
        set("USERPROFILE", row["home"].as_str().unwrap());
        set("PATH", "");
        set("ANTHROPIC_OAUTH_TOKEN", "env://USAGE_STATUS_ABSENT");
        let variable = row["input"]["env"].as_str().unwrap();
        if !variable.is_empty() {
            set(variable, "owned-status-token");
        }
        if row["provider"] == "opencode" {
            set("OPENCODE_WORKSPACE_ID", "wrk_owned");
        }
        let home = std::path::Path::new(row["home"].as_str().unwrap());
        let before = file_state(home);
        let provider = match row["provider"].as_str().unwrap() {
            "claude" => super::super::claude(),
            "codex" => super::super::codex(),
            "copilot" => super::super::copilot(),
            "cursor" => super::super::cursor(),
            "kimi" => super::super::kimi(),
            "moonshot" => super::super::moonshot(),
            "nous" => super::super::nous(),
            "opencode" => super::super::opencode(),
            "openrouter" => super::super::openrouter(),
            _ => panic!("complete remote registry"),
        };
        let transport = Arc::new(super::wire::wired(row));
        let start = chrono::Utc::now();
        let report = Service::with_transport(vec![provider], transport.clone()).report();
        let end = chrono::Utc::now();
        let raw = serde_json::to_value(&report).unwrap();
        assert_eq!(
            report_value(report, start, end),
            expected_report(row),
            "{} actual remote full status/report",
            row["id"]
        );
        let requests = transport.requests.lock().unwrap().clone();
        let mut go = normalize_requests(&row["requests"]);
        let mut native = json!(requests);
        // Preserve both runtime-generated Go UUIDs and existing native fixed
        // instance; inherited default identity is not part of this status fix.
        for values in [&mut go, &mut native] {
            for request in values.as_array_mut().unwrap() {
                for field in ["headers", "header_value_hex"] {
                    if let Some(headers) = request[field].as_object_mut() {
                        headers.remove("x-server-instance");
                    }
                }
            }
        }
        assert_eq!(
            native, go,
            "{} full logical request/rawheaders except retained OpenCode instance difference",
            row["id"]
        );
        assert_eq!(
            file_state(home),
            before,
            "{} owned metadata/bytes untouched",
            row["id"]
        );
        observed.push(json!({"id":row["id"],"report":raw,"started":start,"finished":end,"requests":requests,"filesystem":before,"read_only":true}));
    }
    let output = json!({"cases":84,"records":observed,"full_reports":84,"read_only":84});
    std::fs::write(
        std::env::var("USAGE_STATUS_NATIVE").unwrap(),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
    output
}
