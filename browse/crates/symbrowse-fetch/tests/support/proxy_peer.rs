//! Owned wire phases and socket closure, with an observed, abort-on-drop task.
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    task::JoinHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    WithheldHeaders,
    PartialBody,
    CompleteBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    HeadersRead,
    PartialBodySent,
    CompleteBodySent,
    ClosedDuringHeaders,
}

#[derive(Debug)]
pub struct Observation {
    pub phase: Phase,
    pub header: Vec<u8>,
}

pub struct Peer {
    pub url: String,
    ready: Option<oneshot::Receiver<Observation>>,
    closed: Option<oneshot::Receiver<Observation>>,
    task: Option<JoinHandle<Result<(), String>>>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

fn disconnected(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    )
}

impl Peer {
    pub async fn spawn(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ready, started) = oneshot::channel();
        let (closed, ended) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.map_err(|error| error.to_string())?;
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                let read = socket.read(&mut byte).await;
                match read {
                    Ok(1) => header.push(byte[0]),
                    Ok(0) => {
                        let _ = ready.send(Observation {
                            phase: Phase::ClosedDuringHeaders,
                            header: header.clone(),
                        });
                        let _ = closed.send(Observation {
                            phase: Phase::ClosedDuringHeaders,
                            header,
                        });
                        return Ok(());
                    }
                    Err(error) if disconnected(&error) => {
                        let _ = ready.send(Observation {
                            phase: Phase::ClosedDuringHeaders,
                            header: header.clone(),
                        });
                        let _ = closed.send(Observation {
                            phase: Phase::ClosedDuringHeaders,
                            header,
                        });
                        return Ok(());
                    }
                    Err(error) => return Err(error.to_string()),
                    _ => return Err("unexpected one-byte read size".into()),
                }
                if header.len() >= 8192 {
                    return Err("owned proxy header bound exceeded".into());
                }
            }
            if !header.starts_with(b"GET http://93.184.216.34:080/body HTTP/1.1\r\n") {
                return Err(format!("unexpected raw proxy request: {header:?}"));
            }
            let phase = match reply {
                Reply::WithheldHeaders => Phase::HeadersRead,
                Reply::PartialBody | Reply::CompleteBody => {
                    let data: &[u8] = if reply == Reply::PartialBody {
                        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n1"
                    } else {
                        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n12345678"
                    };
                    socket
                        .write_all(data)
                        .await
                        .map_err(|error| error.to_string())?;
                    if reply == Reply::PartialBody {
                        Phase::PartialBodySent
                    } else {
                        Phase::CompleteBodySent
                    }
                }
            };
            let _ = ready.send(Observation {
                phase,
                header: header.clone(),
            });
            let mut byte = [0];
            match socket.read(&mut byte).await {
                Ok(0) => {}
                Err(error) if disconnected(&error) => {}
                read => {
                    return Err(format!(
                        "owned proxy did not observe peer closure: {read:?}"
                    ));
                }
            }
            let _ = closed.send(Observation { phase, header });
            Ok(())
        });
        Self {
            url: format!("http://{address}"),
            ready: Some(started),
            closed: Some(ended),
            task: Some(task),
        }
    }

    pub async fn ready(&mut self, phase: Phase) -> Observation {
        let observed = tokio::time::timeout(Duration::from_secs(2), self.ready.take().unwrap())
            .await
            .expect("proxy phase admission timed out")
            .expect("proxy phase task failed");
        assert_eq!(observed.phase, phase, "unexpected proxy admission phase");
        observed
    }

    pub async fn observe_close(&mut self, budget: Duration) -> Option<Observation> {
        match tokio::time::timeout(budget, self.closed.as_mut().unwrap()).await {
            Ok(result) => {
                self.closed.take();
                Some(result.expect("owned proxy close task failed"))
            }
            Err(_) => None,
        }
    }

    pub async fn assert_closed(&mut self, phase: Phase) {
        let observation = self
            .observe_close(Duration::from_secs(2))
            .await
            .expect("owned proxy remains connected");
        assert_eq!(observation.phase, phase);
        let task = self.task.take().unwrap();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("owned proxy task did not join")
            .expect("owned proxy task panicked")
            .expect("owned proxy task failed");
    }
}
