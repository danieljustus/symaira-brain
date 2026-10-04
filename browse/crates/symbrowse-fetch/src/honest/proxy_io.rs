//! The raw-URI path dials only the already policy-checked selected proxy.
use super::PinnedResolver;
use crate::{FetchClient, FetchError};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use url::Url;

pub(super) trait ProxyIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ProxyIo for T {}

pub(super) async fn connect(
    client: &FetchClient,
    proxy: &Url,
    protected: bool,
) -> Result<Box<dyn ProxyIo>, FetchError> {
    let host = proxy
        .host_str()
        .expect("policy-checked proxy host")
        .trim_matches(['[', ']']);
    let port = proxy.port_or_known_default().expect("HTTP(S) proxy port");
    let tcp = if protected {
        let resolver = client
            .resolver
            .clone()
            .unwrap_or_else(PinnedResolver::system);
        let addresses: Vec<SocketAddr> = resolver
            .resolve_for_policy(host)
            .map_err(FetchError::ProxyTransport)?
            .into_iter()
            .map(|mut address| {
                address.set_port(port);
                address
            })
            .collect();
        TcpStream::connect(addresses.as_slice()).await
    } else {
        TcpStream::connect((host, port)).await
    }
    .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
    tcp.set_nodelay(true)
        .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
    if proxy.scheme() == "https" {
        // The same native TLS backend/default trust and hostname checks as reqwest.
        let tls = native_tls::TlsConnector::new()
            .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
        let tls = tokio_native_tls::TlsConnector::from(tls)
            .connect(host, tcp)
            .await
            .map_err(|error| FetchError::ProxyTransport(error.to_string()))?;
        Ok(Box::new(tls))
    } else {
        Ok(Box::new(tcp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn protected_raw_proxy_dial_uses_the_shared_resolver_and_rejects_private_result() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let called = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&called);
        let address = listener.local_addr().unwrap();
        let client = FetchClient::honest()
            .unwrap()
            .with_resolver(PinnedResolver::with_lookup(move |host| {
                assert_eq!(host, "owned-proxy.test");
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(vec![address])
            }));
        let proxy = Url::parse(&format!("http://owned-proxy.test:{}", address.port())).unwrap();
        assert!(matches!(
            connect(&client, &proxy, true).await,
            Err(FetchError::ProxyTransport(_))
        ));
        assert_eq!(called.load(Ordering::SeqCst), 1);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    }
}
