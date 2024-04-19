use clap::Parser;
use mredis::config::{parse_log_level, Config};
use mredis::server::Server;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    let cfg = Config::parse();
    let log_level = parse_log_level(cfg.verbosity);
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(LevelFilter::from(log_level))
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("unable to initialize logging");

    let server = Server::new(&cfg).await;
    server.listen().await;
    Ok(())
}
