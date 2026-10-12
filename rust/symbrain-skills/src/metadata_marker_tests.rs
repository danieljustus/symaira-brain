use super::decode_marker;

#[test]
fn metadata_marker_has_its_own_four_string_fields_and_null_duplicate_rules() {
    let marker = decode_marker(br#"{"schema_version":{},"installed":"old","installed":"new","installed":null,"rendered_at":"tree","unknown":10000000000000000000000000000000000000000}"#).unwrap();
    assert_eq!(marker.installed, "new");
    assert_eq!(marker.rendered_at, "tree");
    assert!(decode_marker(br#"{"target":4,"installed":"new"}"#).is_none());
    assert!(decode_marker(br#"{"name":4,"name":"repaired","installed":"new"}"#).is_none());
    assert!(decode_marker(br#"{"installed":"new""#).is_none());
}

#[test]
fn metadata_marker_repairs_each_invalid_byte_before_presentation() {
    let marker = decode_marker(b"{\"installed\":\"\xe2\x82\"}").unwrap();
    assert_eq!(marker.installed, "\u{fffd}\u{fffd}");
}
