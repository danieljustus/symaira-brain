//! Bounded explicit loopback listener and graceful ownership of connection tasks.

use super::{Server, log};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, future::Future, net::TcpListener, sync::Arc, time::Duration};

impl Server {
    /// Runs an explicitly bound loopback listener until its owner cancels.
    ///
    /// The owner supplies the existing runtime and the shutdown signal; this
    /// method opens no store, resolves no credentials and starts no browser.
    ///
    /// # Errors
    /// Rejects non-loopback listeners or returns an accept/runtime I/O error.
    pub async fn serve_until(
        self: Arc<Self>,
        listener: TcpListener,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<(), std::io::Error> {
        if !listener.local_addr()?.ip().is_loopback() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "memory HTTP requires a loopback listener",
            ));
        }
        self.listener_port.store(
            listener.local_addr()?.port(),
            std::sync::atomic::Ordering::Relaxed,
        );
        listener.set_nonblocking(true)?;
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let mut tasks = tokio::task::JoinSet::new();
        let (stop, watch) = tokio::sync::watch::channel(false);
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                ()=&mut shutdown=>break,
                result=listener.accept()=> {
                    let (stream,peer)=result?;
                    if tasks.len()>=128 {drop(stream);continue;}
                    let server=Arc::clone(&self);let mut watch=watch.clone();
                    tasks.spawn(async move {
                        let service=hyper::service::service_fn(move|request| {
                            let server=Arc::clone(&server);
                            async move {Ok::<_,Infallible>(server.handle(request,peer.ip()).await)}
                        });
                        let mut builder=hyper::server::conn::http1::Builder::new();
                        builder.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(5)).max_buf_size(1<<20);
                        let connection=builder.serve_connection(TokioIo::new(stream),service);
                        tokio::pin!(connection);
                        tokio::select! {
                            _=&mut connection=>{},
                            _=watch.changed()=> {
                                connection.as_mut().graceful_shutdown();
                                let _=tokio::time::timeout(Duration::from_secs(5),&mut connection).await;
                            }
                        }
                    });
                },
                _=tasks.join_next(),if !tasks.is_empty()=>{},
            }
        }
        drop(listener);
        let _ = stop.send(true);
        if tokio::time::timeout(Duration::from_secs(5), async {
            while tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
        Ok(())
    }
}

/// Go's wording of the `net.Listen` failures a CLI user can hit.
fn go_listen_reason(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(48 | 98) => "address already in use".to_owned(),
        Some(13) => "permission denied".to_owned(),
        Some(49 | 99) => "cannot assign requested address".to_owned(),
        _ => {
            let text = error.to_string();
            let base = text.split(" (os error").next().unwrap_or(&text);
            let mut chars = base.chars();
            chars
                .next()
                .map(|first| first.to_lowercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}

impl Server {
    /// `StartHTTPServer`: binds `127.0.0.1:port`, logs the listening line,
    /// serves until SIGINT/SIGTERM and then shuts down gracefully.
    ///
    /// # Errors
    /// Returns the shipped `failed to bind to …` wording when the port is not
    /// available, or the runtime/accept error.
    pub fn run_loopback(self: Arc<Self>, port: u16) -> Result<(), String> {
        let address = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&address).map_err(|error| {
            format!(
                "failed to bind to {address}: listen tcp {address}: bind: {}",
                go_listen_reason(&error)
            )
        })?;
        log::info(
            "Symaira Memory API listening",
            &[
                ("addr", address.clone()),
                ("url", format!("http://{address}")),
            ],
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
            let mut interrupt = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
            self.serve_until(listener, async move {
                tokio::select! {
                    _ = terminate.recv() => {},
                    _ = interrupt.recv() => {},
                }
            })
            .await
            .map_err(|error| error.to_string())
        })
    }
}
