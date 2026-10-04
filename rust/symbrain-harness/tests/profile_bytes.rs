//! Format-specific profile bytes and byte-valued diff boundaries.
use symbrain_core::GoText;
use symbrain_harness::{Entry, empty, lookup, parse, unified_diff_bytes};

#[test]
fn codex_retains_raw_profile_and_does_not_replace_equal_collateral_values() {
    let harness = lookup("codex").unwrap();
    let mut doc = parse(harness, "title=\"owned�\"\n[mcp_servers.other]\ncommand=\"owned�\"\nargs=[\"mcp\",\"--profile\",\"owned�\"]\n".as_bytes()).unwrap();
    doc.set_profile_server("symbrain", &GoText::from(b"owned\xff".to_vec()));
    doc.set_profile_server("another", &GoText::from(b"owned\xe2\x82".to_vec()));
    let output = doc.marshal().unwrap();
    assert!(
        output
            .windows(b"\"owned\xff\"".len())
            .any(|part| part == b"\"owned\xff\"")
    );
    assert!(
        output
            .windows(b"\"owned\xe2\x82\"".len())
            .any(|part| part == b"\"owned\xe2\x82\"")
    );
    assert!(
        output
            .windows("title=\"owned�\"".len())
            .any(|part| part == "title=\"owned�\"".as_bytes())
    );
    assert_eq!(doc.server("other").unwrap().profile(), Some("owned�"));
    assert!(doc.remove_server("another"));
    doc.set_server("symbrain", Entry::new("plain"));
    let plain = doc.marshal().unwrap();
    assert!(!plain.contains(&0xff));
    assert!(parse(harness, &plain).is_ok());
}

#[test]
fn json_repairs_each_invalid_byte_without_html_or_javascript_reencoding() {
    let mut doc = empty(lookup("claude").unwrap());
    doc.set_profile_server("symbrain", &GoText::from(b"<owned>\xe2\x82".to_vec()));
    let output = doc.marshal().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["mcpServers"]["symbrain"]["args"][2], "<owned>��");
    assert!(
        output
            .windows(b"<owned>".len())
            .any(|part| part == b"<owned>")
    );
}

#[test]
fn raw_diff_distinguishes_invalid_bytes_from_literal_replacement_rune() {
    let output = unified_diff_bytes(b"path\xff", b"owned\xe2\x82\n", "owned��\n".as_bytes());
    assert_eq!(output, b"--- path\xff\n+++ path\xff\n@@ -1,1 +1,1 @@\n-owned\xe2\x82\n+owned\xef\xbf\xbd\xef\xbf\xbd\n");
    assert!(unified_diff_bytes(b"path\xff", b"owned\xff\n", b"owned\xff\n").is_empty());
}
