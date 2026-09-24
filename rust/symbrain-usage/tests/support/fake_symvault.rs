use std::fs::OpenOptions;
use std::io::{self, Write};
use std::process;
use std::thread;
use std::time::Duration;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn main() {
    let args_path = env("SECRET_ORACLE_FAKE_ARGS_PATH");
    if args_path.is_empty() {
        process::exit(127);
    }

    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let mut log = match OpenOptions::new().create(true).append(true).open(args_path) {
        Ok(file) => file,
        Err(_) => process::exit(127),
    };
    if arguments.is_empty() {
        let _ = log.write_all(b"\n");
    } else {
        for argument in arguments {
            let _ = writeln!(log, "{argument}");
        }
    }

    let stdout_bytes = env("SECRET_ORACLE_FAKE_STDOUT_BYTES");
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(env("SECRET_ORACLE_FAKE_STDOUT").as_bytes());
    if !stdout_bytes.is_empty() {
        for byte in stdout_bytes.split(',') {
            match byte.parse::<u8>() {
                Ok(value) => {
                    let _ = stdout.write_all(&[value]);
                }
                Err(_) => process::exit(127),
            }
        }
    }
    let _ = stdout.flush();
    let _ = io::stderr()
        .lock()
        .write_all(env("SECRET_ORACLE_FAKE_STDERR").as_bytes());
    let exit = env("SECRET_ORACLE_FAKE_EXIT").parse::<i32>().unwrap_or(127);
    let sleep_ms = env("SECRET_ORACLE_FAKE_SLEEP_MS")
        .parse::<u64>()
        .unwrap_or(0);
    if sleep_ms > 0 {
        thread::sleep(Duration::from_millis(sleep_ms));
        process::exit(0);
    }
    process::exit(exit);
}
