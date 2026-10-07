use super::GoWriter;
use std::io::{self, Write};

struct Recovering {
    calls: Vec<Vec<u8>>,
    accepted: Vec<u8>,
    first: io::Result<usize>,
}

impl Write for Recovering {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls.push(bytes.to_vec());
        if self.calls.len() == 1 {
            let result = std::mem::replace(&mut self.first, Ok(0));
            if let Ok(count) = result {
                self.accepted.extend_from_slice(&bytes[..count]);
            }
            result
        } else {
            self.accepted.extend_from_slice(bytes);
            Ok(bytes.len())
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn ignored_fmt_failures_continue_at_the_next_whole_operation() {
    let outcomes = [
        Ok(2),
        Ok(0),
        Err(io::Error::other("owned callback")),
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "owned callback")),
        Err(io::Error::from_raw_os_error(32)),
        Err(io::Error::from_raw_os_error(28)),
    ];
    for outcome in outcomes {
        let prefix = match &outcome {
            Ok(count) => *count,
            Err(_) => 0,
        };
        let mut writer = Recovering {
            calls: Vec::new(),
            accepted: Vec::new(),
            first: outcome,
        };
        let mut go = GoWriter::new(&mut writer);
        let word = "operation";
        writeln!(go, "first {word}").unwrap();
        writeln!(go, "second {word}").unwrap();
        assert_eq!(
            writer.calls,
            [
                b"first operation\n".to_vec(),
                b"second operation\n".to_vec()
            ]
        );
        let mut expected = b"first operation\n"[..prefix].to_vec();
        expected.extend_from_slice(b"second operation\n");
        assert_eq!(writer.accepted, expected);
    }
}
