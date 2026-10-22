//! Expected values were produced by running Go's own `filepath.Clean` Windows
//! source (`internal/filepathlite`, go1.26.6: `Clean`, `lazybuf`,
//! `volumeNameLen`, `postClean` copied verbatim) on the inputs below. The
//! builder is pure byte logic, so these run on every host; native Windows
//! evidence for the CLI messages comes from the `windows-2025` jobs.
use super::{clean_windows, printed};
use std::path::Path;

/// `(input, filepath.Clean output on Windows)`.
const GO_WINDOWS_CLEAN: &[(&str, &str)] = &[
    (
        r"D:\a\_temp\.tmpF51FVv\bad-config-0\config\symbrain\config.toml",
        r"D:\a\_temp\.tmpF51FVv\bad-config-0\config\symbrain\config.toml",
    ),
    (
        r"D:\a\_temp\.tmpF51FVv\bad-config-0\config/symbrain/config.toml",
        r"D:\a\_temp\.tmpF51FVv\bad-config-0\config\symbrain\config.toml",
    ),
    (
        r"C:/Users/me/.config\symbrain\config.toml",
        r"C:\Users\me\.config\symbrain\config.toml",
    ),
    (
        r"C:/Users/me/.config/symbrain/config.toml",
        r"C:\Users\me\.config\symbrain\config.toml",
    ),
    (
        r"C:\cfg\\symbrain\.\config.toml",
        r"C:\cfg\symbrain\config.toml",
    ),
    (
        r"C:\a\b\..\c\symbrain\config.toml",
        r"C:\a\c\symbrain\config.toml",
    ),
    (r"C:\cfg\symbrain\..\..\x\config.toml", r"C:\x\config.toml"),
    (
        r"\\host\share\cfg\symbrain\config.toml",
        r"\\host\share\cfg\symbrain\config.toml",
    ),
    (
        r"//host/share/cfg/symbrain/config.toml",
        r"\\host\share\cfg\symbrain\config.toml",
    ),
    (r"\\host\share\a\..\..\..\x", r"\\host\share\x"),
    (
        r"\\?\C:\cfg\symbrain\config.toml",
        r"\\?\C:\cfg\symbrain\config.toml",
    ),
    (
        r"\\?\C:/cfg/symbrain/config.toml",
        r"\\?\C:\cfg\symbrain\config.toml",
    ),
    (
        r".config\symbrain\config.toml",
        r".config\symbrain\config.toml",
    ),
    (
        r".config/symbrain/config.toml",
        r".config\symbrain\config.toml",
    ),
    (r"C:", r"C:."),
    (r"C:\", r"C:\"),
    (r"C:/", r"C:\"),
    (r"C:\..\x", r"C:\x"),
    (r"C:\cfg\", r"C:\cfg"),
    (r"C:\cfg\.", r"C:\cfg"),
    (r"C:\..", r"C:\"),
    (r"C:\cfg\..\..", r"C:\"),
    (r"ab:c", r"ab:c"),
    (r"a/../c:x", r".\c:x"),
    (r"a/../c:/x", r".\c:\x"),
    (r"a\..\c:\x", r".\c:\x"),
    (r"\??\C:\x", r"\??\C:\x"),
    (r"\a\..\??\c:\x", r"\.\??\c:\x"),
    (
        r"C:/Users/Jürgen/.config/symbrain/config.toml",
        r"C:\Users\Jürgen\.config\symbrain\config.toml",
    ),
    (r"\\.\UNC\host\share\a/../b", r"\\.\UNC\host\share\b"),
    (r"\\.\C:\x", r"\\.\C:\x"),
    (r"//./C:/x", r"\\.\C:\x"),
    (r"\\.", r"\\."),
    (r"\\?", r"\\?"),
    (r"\\host", r"\\host"),
    (r"\\host\share", r"\\host\share"),
    (r"", r"."),
    (r".", r"."),
    (r"./", r"."),
    (r"..", r".."),
    (r"..\a", r"..\a"),
    (r"../a/../..", r"..\.."),
    (r"a/b/../../..", r".."),
    (r"\x", r"\x"),
    (r"/x/y/../z", r"\x\z"),
];

#[test]
fn matches_go_windows_clean_for_every_recorded_input() {
    for (input, expected) in GO_WINDOWS_CLEAN {
        assert_eq!(
            String::from_utf8(clean_windows(input.as_bytes())).unwrap(),
            *expected,
            "{input:?}"
        );
    }
}

#[test]
fn cleaning_a_cleaned_path_changes_nothing() {
    for (_, expected) in GO_WINDOWS_CLEAN {
        let once = clean_windows(expected.as_bytes());
        assert_eq!(clean_windows(&once), once, "{expected:?}");
    }
}

#[test]
fn mixed_separator_config_path_prints_with_backslashes() {
    // The shape the native CLI tests build with `case.join("config/symbrain/config.toml")`:
    // Go prints every separator as `\` on Windows.
    assert_eq!(
        clean_windows(br"D:\a\_temp\.tmpF51FVv\bad-config-0\config/symbrain/config.toml"),
        br"D:\a\_temp\.tmpF51FVv\bad-config-0\config\symbrain\config.toml"
    );
    assert_eq!(
        clean_windows(br"C:/Users/me/.config\symbrain\config.toml"),
        br"C:\Users\me\.config\symbrain\config.toml"
    );
}

#[test]
fn printed_changes_nothing_off_windows_and_cleans_on_windows() {
    let unclean = Path::new("/tmp//a/./b/../c/symbrain/config.toml");
    let shown = printed(unclean);
    if cfg!(windows) {
        assert_eq!(shown, clean_windows(unclean.as_os_str().as_encoded_bytes()));
    } else {
        assert_eq!(shown, b"/tmp//a/./b/../c/symbrain/config.toml");
    }
}
