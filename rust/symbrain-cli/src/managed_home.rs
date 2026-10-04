//! The managed owner uses shared byte-preserving Go lexical Join/Clean.
use std::path::PathBuf;

pub(crate) fn bin_dir() -> Option<PathBuf> {
    let home = std::env::var_os(symbrain_core::go_path::home_variable())
        .filter(|home| !home.is_empty())?;
    Some(managed_path(&home))
}
fn managed_path(home: &std::ffi::OsStr) -> PathBuf {
    symbrain_core::go_path::join(&[
        home,
        std::ffi::OsStr::new(".symaira"),
        std::ffi::OsStr::new("bin"),
    ])
}
#[cfg(test)]
use symbrain_core::go_path::clean_bytes as clean;

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn lexical_clean_preserves_unix_bytes_and_relative_parents() {
        for (input, expected) in [
            (
                b"/root/link/../home//./.symaira/bin".as_slice(),
                b"/root/home/.symaira/bin".as_slice(),
            ),
            (
                b"../../home/../raw\xff\xe2\x82/.symaira/bin",
                b"../../raw\xff\xe2\x82/.symaira/bin",
            ),
            (b"/../../home/.symaira/bin", b"/home/.symaira/bin"),
        ] {
            assert_eq!(clean(input, false), expected);
        }
    }

    #[test]
    fn lexical_windows_drive_unc_and_device_roots_follow_sdk() {
        for (input, expected) in [
            (
                br"C:\link\..\home\.symaira\bin".as_slice(),
                br"C:\home\.symaira\bin".as_slice(),
            ),
            (br"C:..\home\.symaira\bin", br"C:..\home\.symaira\bin"),
            (
                br"\\host\share\..\home\.symaira\bin",
                br"\\host\share\home\.symaira\bin",
            ),
            (
                br"\\?\C:\..\home\.symaira\bin",
                br"\\?\C:\home\.symaira\bin",
            ),
            (br"a\..\c:home\.symaira\bin", br".\c:home\.symaira\bin"),
            (br"\a\..\??\c:\home", br"\.\??\c:\home"),
            (br"\\host\share", br"\\host\share"),
        ] {
            assert_eq!(clean(input, true), expected);
        }
    }

    #[test]
    #[cfg(windows)]
    fn managed_join_keeps_drive_relative_and_unc_ownership() {
        use std::ffi::OsStr;
        for (home, expected) in [
            (r"C:", r"C:.symaira\bin"),
            (r"C:\home\..\owner", r"C:\owner\.symaira\bin"),
            (r"\\host\share\..\owner", r"\\host\share\owner\.symaira\bin"),
            (r"\\?\C:\home\..\owner", r"\\?\C:\owner\.symaira\bin"),
        ] {
            assert_eq!(
                super::managed_path(OsStr::new(home)).as_os_str(),
                OsStr::new(expected)
            );
        }
    }
}
