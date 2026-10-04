use super::{Output, go_write_error, io_cause};
use std::io::{self, Write};

struct Broken(bool);
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        if self.0 {
            #[cfg(unix)]
            let code = 32;
            #[cfg(not(unix))]
            let code = 109;
            Err(io::Error::from_raw_os_error(code))
        } else {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "owned callback"))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn embedded_raw_and_kind_only_pipe_errors_are_ordinary_errors() {
    for raw in [false, true] {
        let error = Output::new(&mut Broken(raw), false)
            .write_all(b"owned")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        if raw {
            assert!(error.raw_os_error().is_some());
            #[cfg(unix)]
            assert_eq!(io_cause(&error), "broken pipe");
        } else {
            assert_eq!(io_cause(&error), "owned callback");
        }
    }
    // A callback's ErrorKind does not prove an actual OS EPIPE, even when the
    // narrow process seam is enabled. This test must never terminate its host.
    let error = Output::new(&mut Broken(false), true)
        .write_all(b"owned")
        .unwrap_err();
    assert_eq!(error.to_string(), "owned callback");
}

#[cfg(unix)]
#[test]
fn real_stdout_io_error_retains_operation_path_cause_and_source() {
    struct Full;
    impl Write for Full {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from_raw_os_error(28))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let error = Output::new(&mut Full, true)
        .write_all(b"owned")
        .unwrap_err();
    assert_eq!(
        io_cause(&error),
        "write /dev/stdout: no space left on device"
    );
    assert_eq!(
        go_write_error(&error),
        "write /dev/stdout: no space left on device"
    );
    let nested = error.get_ref().unwrap();
    assert!(nested.source().is_some());
}
