//! Guard version handshake; preserve the actual Go command's wire bytes.
use std::ffi::OsString;
use std::io::{self, Write};
use symbrain_core::{exit, version};
use symbrain_guard_core::go_json::to_go_json_vec;

pub(super) fn run(args: &[OsString], stdout: &mut dyn Write) -> u8 {
    let version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
    let info = version::VersionInfo::new("symguard", version);
    let result = if args.iter().any(|arg| arg == "--json") {
        to_go_json_vec(&info)
            .map_err(io::Error::other)
            .and_then(|mut bytes| {
                // versionkit.Write includes a newline; the Go Guard command adds one.
                bytes.extend_from_slice(b"\n\n");
                stdout.write_all(&bytes)
            })
    } else {
        (|| {
            writeln!(stdout, "symguard {version}")?;
            writeln!(stdout, "  rust    {}", crate::rustc_version())?;
            writeln!(
                stdout,
                "  os/arch {}/{}",
                version::current_os(),
                version::current_arch()
            )?;
            writeln!(stdout, "  built   2026-01-01 (compile-time placeholder)")
        })()
    };
    if result.is_ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn version_output_failures_are_nonzero_in_both_formats() {
        assert_eq!(run(&[], &mut Broken), exit::GENERIC);
        assert_eq!(run(&["--json".into()], &mut Broken), exit::GENERIC);
    }
}
