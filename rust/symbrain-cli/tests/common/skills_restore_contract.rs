//! #918: explicit security-contract exceptions to the frozen, vulnerable Go oracle.
//! No fixture is repinned and no case is skipped: rejection bytes and no writes
//! are checked instead. All other replies (including history) remain byte-exact.

use serde_json::{Value, json};

pub(super) const CASES: [&str; 13] = [
    "restore-name-traversal",
    "restore-absolute-name-dry-run-victim-only",
    "restore-absolute-name-explicit-dry-run-victim-only",
    "restore-absolute-name-real-victim-only",
    "restore-absolute-name-real-dirty-allowed-victim-only",
    "restore-absolute-name-dry-run-rerooted-also-exists",
    "restore-absolute-name-explicit-dry-run-rerooted-also-exists",
    "restore-absolute-name-real-rerooted-also-exists",
    "restore-absolute-name-real-dirty-allowed-rerooted-also-exists",
    "restore-absolute-name-root-slash",
    "restore-absolute-name-etc",
    "restore-absolute-name-library-alpha",
    "restore-absolute-name-double-slash",
];

pub(super) fn expected(case: &Value, before: &Value, root: &std::path::Path) -> Value {
    if !CASES.contains(&case["id"].as_str().unwrap()) {
        return case.clone();
    }
    let request: Value = serde_json::from_str(case["requests"][0].as_str().unwrap()).unwrap();
    assert_eq!(request["params"]["name"], "skills_restore");
    let name = request["params"]["arguments"]["name"].as_str().unwrap();
    let name = name.replace("<ROOT>", &root.display().to_string());
    // This is the existing Go skill.ValidateSkillName error, not a parser residual.
    let quoted = symbrain_skills::gotext::quote(&name);
    let message = if name.len() > 64 {
        format!("restore skill: skill name {quoted} exceeds maximum length of 64 characters")
    } else {
        format!(
            "restore skill: skill name {quoted} must be a single lowercase alphanumeric-dash segment without consecutive, leading, or trailing hyphens"
        )
    }
    .replace(&root.display().to_string(), "<ROOT>");
    let reply = json!({
        "jsonrpc": "2.0", "id": request["id"],
        "result": {
            "_meta": {"symaira.dev/tool_error": {"code": "validation", "message": message}},
            "content": [{"text": message, "type": "text"}], "isError": true
        }
    });
    // Go's wire envelope is ordered jsonrpc,id,result, not sorted map order.
    let reply = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
        reply["id"], reply["result"]
    );
    let mut expected = case.clone();
    expected["go"]["replies"][0] = json!(reply);
    expected["go"]["after"] = before.clone();
    if case["id"] == "restore-absolute-name-real-rerooted-also-exists" {
        // The following history read can no longer include the refused restore's
        // forward commit. Keep every byte of the earlier recorded history.
        let mut history: Value =
            serde_json::from_str(case["go"]["replies"][1].as_str().unwrap()).unwrap();
        let text = history["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["history"][0]["operation"], "restore");
        assert!(
            parsed["history"][0]["subject"]
                .as_str()
                .unwrap()
                .starts_with("restore: skill /zz/victim to ")
        );
        let start = text.find('[').unwrap() + 1;
        let end = text.find("},{").unwrap() + 2;
        let text = format!("{}{}", &text[..start], &text[end..]);
        history["result"]["content"][0]["text"] = json!(text);
        expected["go"]["replies"][1] = json!(format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}",
            history["id"], history["result"]
        ));
    }
    expected
}

#[test]
fn security_exceptions_are_explicit_and_fail_closed() {
    let oracle = super::fixture();
    assert_eq!(
        oracle["pinned_ref"],
        "3d9a89e7a2ee201d93c74501a10eb2af3f87e620"
    );
    let cases = oracle["cases"].as_array().unwrap();
    for id in CASES {
        assert_eq!(cases.iter().filter(|case| case["id"] == id).count(), 1);
    }
    let case = cases.iter().find(|case| case["id"] == CASES[0]).unwrap();
    let mut observed = super::replay(&oracle, case, &super::requests(case));
    let expected = expected(case, &observed.before, &observed.root);
    assert!(
        super::compare(case, &observed).is_err(),
        "frozen Go defect must differ"
    );
    assert!(super::compare(&expected, &observed).is_ok());
    observed.after["entries"]["victim"] = json!({"type": "dir", "mode": 493});
    assert!(
        super::compare(&expected, &observed).is_err(),
        "writes must fail"
    );
    observed.after = observed.before.clone();
    observed.replies[0] = observed.replies[0].replace("validation", "internal");
    assert!(
        super::compare(&expected, &observed).is_err(),
        "error class must match"
    );
}
