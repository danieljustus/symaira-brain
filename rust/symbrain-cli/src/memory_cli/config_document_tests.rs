//! Prepared byte-admission tests for the separate Memory field owner.
use super::{Config, merge_bytes};

#[test]
fn memory_fields_survive_exact_leading_markers() {
    let fields = b"[database]\npath=\"owned-selected.db\"\n[jwt]\nsecret=\"owned-synthetic-key\"\n";
    for prefix in [b"".as_slice(), b"\xff\xfe", b"\xfe\xff", b"\xef\xbb\xbf"] {
        let bytes = [prefix, fields].concat();
        let mut config = Config::default();
        assert_eq!(merge_bytes(&mut config, &bytes), Some(()));
        assert_eq!(config.database, "owned-selected.db");
        assert_eq!(config.jwt_secret, "owned-synthetic-key");
    }
}

#[test]
fn markers_do_not_admit_utf16_or_repaired_utf8_or_wrong_types() {
    for bytes in [
        b"\xff\xfe[\0j\0w\0".as_slice(),
        b"\xfe\xff[jwt]\nsecret=\"owned-\xff\"\n",
        b"\xff\xfe[jwt]\nsecret=[]\n",
        b"\xff\xfe\xfe\xff[jwt]\nsecret=\"unused\"\n",
    ] {
        assert_eq!(merge_bytes(&mut Config::default(), bytes), None);
    }
}
