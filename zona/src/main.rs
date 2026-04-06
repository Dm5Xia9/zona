use std::net::SocketAddr;
use tracing::info;

const DEFAULT_LISTEN: &str = "127.0.0.1:8787";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let listen = std::env::var("ZONA_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_string());
    let addr: SocketAddr = listen.parse()?;

    info!(name = "Zona", %addr, "starting web server");

    let app = zona::app();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received");
}
