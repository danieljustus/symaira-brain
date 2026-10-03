use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub(super) struct TestServer {
    pub(super) url: String,
    pub(super) requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    pub(super) fn start(routes: BTreeMap<String, Vec<u8>>) -> Self {
        Self::start_with_protocol(routes, false)
    }

    pub(super) fn start_http10(routes: BTreeMap<String, Vec<u8>>) -> Self {
        Self::start_with_protocol(routes, true)
    }

    fn start_with_protocol(routes: BTreeMap<String, Vec<u8>>, http10: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_thread = Arc::clone(&requests);
        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut request_line = String::new();
                        let mut reader = BufReader::new(&mut stream);
                        let _ = reader.read_line(&mut request_line);
                        loop {
                            let mut header = String::new();
                            if reader.read_line(&mut header).unwrap_or(0) == 0
                                || header == "\r\n"
                                || header == "\n"
                            {
                                break;
                            }
                        }
                        drop(reader);
                        let path = request_line
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        requests_thread.lock().unwrap().push(path.to_string());
                        let (status, body) = routes
                            .get(path)
                            .map_or(("404 Not Found", b"asset not found".as_slice()), |body| {
                                ("200 OK", body.as_slice())
                            });
                        let protocol = if http10 { "HTTP/1.0" } else { "HTTP/1.1" };
                        let connection = if http10 { "" } else { "Connection: close\r\n" };
                        write!(
                            stream,
                            "{protocol} {status}\r\nContent-Length: {}\r\n{connection}\r\n",
                            body.len()
                        )
                        .unwrap();
                        // A rejected response body may intentionally remain unread.
                        let _ = stream.write_all(body);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
