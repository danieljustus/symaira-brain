use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

pub(super) const STARTUP_OUTPUT_LIMIT: usize = 4096;

pub(super) async fn capture_startup_output<R>(mut reader: R, captured: Arc<Mutex<Vec<u8>>>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(size) = reader.read(&mut chunk).await else {
            break;
        };
        if size == 0 {
            break;
        }
        if let Ok(mut captured) = captured.lock() {
            captured.extend_from_slice(&chunk[..size]);
            if captured.len() > STARTUP_OUTPUT_LIMIT {
                let excess = captured.len() - STARTUP_OUTPUT_LIMIT;
                captured.drain(..excess);
            }
        }
    }
}

pub(super) fn startup_detail(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    let output = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => return None,
        (false, true) => format!("stdout: {stdout}"),
        (true, false) => format!("stderr: {stderr}"),
        (false, false) => format!("stdout: {stdout}; stderr: {stderr}"),
    };
    Some(format!(
        "Firefox remote agent did not become ready; use a Mozilla Firefox Nightly build with WebDriver BiDi enabled; {output}"
    ))
}

pub(super) fn captured_output(output: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    output.lock().map(|bytes| bytes.clone()).unwrap_or_default()
}
