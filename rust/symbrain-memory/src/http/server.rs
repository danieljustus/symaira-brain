//! Bounded explicit loopback listener and graceful ownership of connection tasks.

use super::Server;
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
