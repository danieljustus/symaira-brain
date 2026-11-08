//! Absolute form of the database directory, with Go's `getwd` error boundary.

use super::{errno_text, go_clean};
use std::path::Path;

/// The `PWD` value Go's `os.Getwd` would keep: absolute and naming the very
/// directory the process is in (#928).
fn logical_pwd() -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::MetadataExt;

    std::env::var_os("PWD")
        .filter(|pwd| {
            Path::new(pwd).is_absolute()
                && matches!(
                    (std::fs::metadata("."), std::fs::metadata(pwd)),
                    (Ok(dot), Ok(named)) if dot.dev() == named.dev() && dot.ino() == named.ino()
                )
        })
        .map(OsStringExt::into_vec)
}

/// Go `filepath.Abs` for the directory walk: absolute input is only cleaned,
/// relative input is joined to the working directory.
///
/// A failing working-directory lookup is returned as Go's `getwd: <errno>`
/// error before anything is created; neither `/` nor a guessed directory may
/// stand in for it (#935).
pub(super) fn absolute_directory(directory: &[u8]) -> Result<Vec<u8>, String> {
    resolve(directory, logical_pwd(), std::env::current_dir)
}

fn resolve(
    directory: &[u8],
    pwd: Option<Vec<u8>>,
    current_dir: impl FnOnce() -> std::io::Result<std::path::PathBuf>,
) -> Result<Vec<u8>, String> {
    use std::os::unix::ffi::OsStringExt;

    if directory.first() == Some(&b'/') {
        return Ok(go_clean(directory));
    }
    let mut joined = match pwd {
        Some(pwd) => pwd,
        None => current_dir()
            .map_err(|error| format!("getwd: {}", errno_text(&error)))?
            .into_os_string()
            .into_vec(),
    };
    joined.push(b'/');
    joined.extend_from_slice(directory);
    Ok(go_clean(&joined))
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::io::Error;
    use std::path::PathBuf;

    fn unresolvable() -> std::io::Result<PathBuf> {
        Err(Error::from_raw_os_error(2))
    }

    #[test]
    fn failing_cwd_lookup_is_the_go_getwd_error_for_every_relative_shape() {
        for directory in [&b"rel/sub"[..], b".", b"../up", b"bare"] {
            assert_eq!(
                resolve(directory, None, unresolvable),
                Err("getwd: no such file or directory".into()),
                "{directory:?}"
            );
        }
    }

    #[test]
    fn valid_logical_pwd_and_absolute_input_skip_the_lookup() {
        let must_not_run = || -> std::io::Result<PathBuf> { panic!("cwd lookup must not run") };
        assert_eq!(
            resolve(b"rel/./sub", Some(b"/logical/cwd".to_vec()), must_not_run),
            Ok(b"/logical/cwd/rel/sub".to_vec())
        );
        assert_eq!(
            resolve(b"/a/./b/../c", None, must_not_run),
            Ok(b"/a/c".to_vec())
        );
    }

    #[test]
    fn resolvable_cwd_is_joined_and_cleaned() {
        assert_eq!(
            resolve(b"x/../y", None, || Ok(PathBuf::from("/cwd"))),
            Ok(b"/cwd/y".to_vec())
        );
    }
}
