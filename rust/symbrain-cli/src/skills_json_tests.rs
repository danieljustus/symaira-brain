//! Prepared shared encoder checks; no execution during source preparation.
#[test]
fn skills_json_preserves_literals_and_escapes_go_js_and_html_boundaries() {
    let value = serde_json::json!({"path":"<>&\u{2028}\u{2029}\u{fffd}"});
    assert_eq!(
        super::go_json(&value),
        "{\"path\":\"\\u003c\\u003e\\u0026\\u2028\\u2029\u{fffd}\"}"
    );
    assert_eq!(value["path"], "<>&\u{2028}\u{2029}\u{fffd}");
}
