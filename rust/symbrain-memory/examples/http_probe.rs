//! Actual native owner seam; the complete `memory serve` CLI remains delegated.
use std::{net::TcpListener, path::PathBuf, sync::Arc};
use symbrain_memory::{
    EmbeddingGenerator, Store,
    http::{Options, Server},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("expected owned database path and loopback port".into());
    }
    let store = Arc::new(Store::open(&PathBuf::from(&args[0]))?);
    let secret = std::env::var("JWT_SECRET_KEY")?.into_bytes();
    let generator =
        EmbeddingGenerator::new(&std::env::var("SYMMEMORY_OLLAMA_URL")?, "nomic-embed-text");
    let options = Options {
        direct_writes: true,
        require_profile: std::env::var("SYMMEMORY_SECURITY_REQUIRE_PROFILE")
            .is_ok_and(|s| s == "true"),
        ..Options::default()
    };
    let server = Arc::new(Server::new(store, secret, generator, options)?);
    let listener = TcpListener::bind(("127.0.0.1", args[1].parse::<u16>()?))?;
    println!("native memory HTTP owner: {}", listener.local_addr()?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(server.serve_until(listener, shutdown()))?;
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _=signal.recv()=>{},_=tokio::signal::ctrl_c()=>{} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
