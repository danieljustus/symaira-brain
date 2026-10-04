//! Existing version owner moved byte-for-byte to keep dispatch below400 lines.
use crate::{normalize_flags, rustc_version};
use std::ffi::OsString;
use std::io::{self, Write};
use symbrain_core::{
    exit,
    output::{self, OutputFormat},
    version::{self, VersionInfo},
};

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let normalized = normalize_flags(args);
    if let Some(first) = normalized.first() {
        let first_str = first.to_string_lossy();
        if first_str == "--" {
            if normalized.len() > 1 {
                let unexpected = args
                    .get(1)
                    .map_or_else(|| "".into(), |a| a.to_string_lossy());
                let _ = writeln!(
                    stderr,
                    "symbrain version: unexpected argument {unexpected:?}"
                );
                return exit::USAGE;
            }
        } else if first_str == "-" {
            let _ = writeln!(stderr, "symbrain version: unexpected argument \"-\"");
            return exit::USAGE;
        } else if first_str.starts_with('-') {
            let trimmed = first_str.trim_start_matches('-');
            let name = match trimmed.split_once('=') {
                Some((k, _)) => k,
                None => trimmed,
            };
            if name == "h" || name == "help" {
                let _ = writeln!(stderr, "Usage of version:");
                return exit::USAGE;
            }
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = writeln!(stderr, "Usage of version:");
            return exit::USAGE;
        } else {
            let _ = writeln!(
                stderr,
                "symbrain version: unexpected argument {first_str:?}"
            );
            return exit::USAGE;
        }
    }

    let version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
    let info = VersionInfo::new("symbrain", version);
    if output::render(&mut *stdout, format, &info, |w| -> io::Result<()> {
        writeln!(w, "symbrain {version}")?;
        writeln!(w, "  rust    {}", rustc_version())?;
        writeln!(
            w,
            "  os/arch {}/{}",
            version::current_os(),
            version::current_arch()
        )
    })
    .is_err()
    {
        let _ = writeln!(stderr, "symbrain version: format output");
        return exit::GENERIC;
    }
    exit::OK
}
