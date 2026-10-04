//! Source-reference Windows lexical cases; actual native CI remains required.
use super::{clean_units, join_units};
fn clean(path: &str) -> String {
    String::from_utf16(&clean_units(&path.encode_utf16().collect::<Vec<_>>())).unwrap()
}
fn join(path: &str, parts: &[&str]) -> String {
    String::from_utf16(&join_units(&path.encode_utf16().collect::<Vec<_>>(), parts)).unwrap()
}

#[test]
fn windows_raw_utf16_paths_keep_lone_surrogates() {
    for raw in [0xd800, 0xdc00] {
        let path = [46, 92, raw, 92, 97, 92, 46, 46, 92, 98];
        assert_eq!(clean_units(&path), [raw, 92, 98]);
        assert_eq!(
            join_units(&[raw, 58], &["symguard", "config.toml"]),
            [
                vec![raw, 58],
                "symguard\\config.toml".encode_utf16().collect()
            ]
            .concat()
        );
    }
}

#[test]
fn windows_generated_joins_preserve_relative_volumes() {
    for (base, expected) in [
        (r"C:", r"C:symguard\config.toml"),
        (r"C:.", r"C:symguard\config.toml"),
        (r"C:foo\..", r"C:symguard\config.toml"),
        (r"C:/", r"C:\symguard\config.toml"),
        (r"foo:", r"foo:symguard\config.toml"),
        (r"a/../c:", r".\c:symguard\config.toml"),
        (r"\\host\share\a\..", r"\\host\share\symguard\config.toml"),
        (r"\\?\C:\a\..", r"\\?\C:\symguard\config.toml"),
        (r"\a\..\??", r"\.\??\symguard\config.toml"),
        (r"\", r"\symguard\config.toml"),
        (r"../..", r"..\..\symguard\config.toml"),
    ] {
        assert_eq!(
            join(base, &["symguard", "config.toml"]),
            expected,
            "{base:?}"
        );
    }
    assert_eq!(join(r"\", &["??", "C:", "a"]), r"\.\??\C:a");
}

#[test]
fn windows_volume_and_device_paths_follow_go_clean() {
    let cases = [
        ("", "."),
        (".", "."),
        ("a/./../b", "b"),
        (r"C:", r"C:."),
        (r"C:\", r"C:\"),
        (r"C:foo\..\..\bar", r"C:..\bar"),
        (
            r"C:/parent/../xdg/symguard/config.toml",
            r"C:\xdg\symguard\config.toml",
        ),
        (r"C:\..\abc", r"C:\abc"),
        (r"\..\abc", r"\abc"),
        (r"\\host\share\foo\..\..\..\bar", r"\\host\share\bar"),
        (r"//host/share/foo/../bar", r"\\host\share\bar"),
        (r"\\.\C:\a\..\..\bar", r"\\.\C:\bar"),
        (
            r"\\?\C:\a\..\symguard\config.toml",
            r"\\?\C:\symguard\config.toml",
        ),
        (r"\\?\UNC\host\share\foo\..\bar", r"\\?\UNC\host\share\bar"),
        (r"\\.\UNC\host\share\..\bar", r"\\.\UNC\host\share\bar"),
        (r"\\?\C:\", r"\\?\C:\"),
        (r"\\host\share", r"\\host\share"),
        (r"\\i\..\c$", r"\\i\..\c$"),
        (r"a/../c:", r".\c:"),
        (r"a/../c:/a", r".\c:\a"),
        (r"a/../../c:", r"..\c:"),
        (r".\c:foo", r".\c:foo"),
        (r"foo:bar", r"foo:bar"),
        (r"foo:bar/", r"foo:bar"),
        (r"foo:bar/a/..", r".\foo:bar"),
        (r"foo:bar\a\..\", r"foo:bar"),
        (r"foo:bar\a\..\b", r".\foo:bar\b"),
        (r"/a/../??/a", r"\.\??\a"),
        (r"\??\C:\a\..\b", r"\??\C:\b"),
        (r"///abc", r"\\\abc"),
        (r"//abc//", r"\\abc\\"),
        (r"C:\雪\..\symguard\config.toml", r"C:\symguard\config.toml"),
    ];
    for (input, expected) in cases {
        assert_eq!(clean(input), expected, "{input:?}");
        assert_eq!(clean(expected), expected, "idempotence {input:?}");
    }
    assert_eq!(join("foo:bar/a/..", &["..", "b"]), r".\b");
    assert_eq!(
        join_units(&[0xdc00, 58, 47, 97, 47, 46, 46], &["..", "b"]),
        ".\\b".encode_utf16().collect::<Vec<_>>()
    );
}
