//! Owned Go-oracle caller: custom writers are never process stdout.
#![deny(unsafe_code)]
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

struct Broken(String);
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        match self.0.as_str() {
            "raw-pipe" => {
                #[cfg(unix)]
                let code = 32;
                #[cfg(not(unix))]
                let code = 109;
                Err(io::Error::from_raw_os_error(code))
            }
            "raw-full" => {
                #[cfg(unix)]
                let code = 28;
                #[cfg(not(unix))]
                let code = 112;
                Err(io::Error::from_raw_os_error(code))
            }
            "kind-pipe" => Err(io::Error::new(io::ErrorKind::BrokenPipe, "owned callback")),
            _ => Err(io::Error::other("owned callback")),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut writer = Broken(std::env::var("SETUP_EMBEDDED_MODE").expect("owned writer mode"));
    ExitCode::from(symbrain_cli::run(&args, &mut writer, &mut io::stderr()))
}
