//! Frozen-Go comparison caller: every writer is embedded, including raw EPIPE.
#![deny(unsafe_code)]
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

struct Owned {
    mode: String,
    remaining: usize,
    recover: bool,
    failed: bool,
}
impl Write for Owned {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed && self.recover {
            io::stdout().write_all(bytes)?;
            return Ok(bytes.len());
        }
        let count = bytes.len().min(self.remaining);
        io::stdout().write_all(&bytes[..count])?;
        self.remaining -= count;
        if self.remaining > 0 {
            return Ok(count);
        }
        self.failed = true;
        Err(match self.mode.as_str() {
            "raw-pipe" => {
                #[cfg(unix)]
                let code = 32;
                #[cfg(not(unix))]
                let code = 109;
                io::Error::from_raw_os_error(code)
            }
            "raw-full" => {
                #[cfg(unix)]
                let code = 28;
                #[cfg(not(unix))]
                let code = 112;
                io::Error::from_raw_os_error(code)
            }
            _ => io::Error::other("owned partial callback"),
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut writer = Owned {
        mode: std::env::var("DOCTOR_WRITER_MODE").expect("owned mode"),
        remaining: std::env::var("DOCTOR_WRITER_BUDGET")
            .unwrap()
            .parse()
            .unwrap(),
        recover: std::env::var("DOCTOR_WRITER_RECOVER").as_deref() == Ok("1"),
        failed: false,
    };
    ExitCode::from(symbrain_cli::run(&args, &mut writer, &mut io::stderr()))
}
