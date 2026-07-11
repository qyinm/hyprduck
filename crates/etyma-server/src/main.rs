use anyhow::Result;
use etyma_server::config::ServerConfig;
use etyma_server::serve;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let config = ServerConfig::from_env()?;
    serve(config).await
}
