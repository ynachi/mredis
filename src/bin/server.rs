use clap::Parser;
use tracing_subscriber::filter::LevelFilter;
use mredis::config::{Config, parse_log_level};
use mredis::server::Server;

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    let cfg = Config::parse();
    let log_level = parse_log_level(cfg.verbosity);
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(LevelFilter::from(log_level))
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("unable to initialize logging");
    
    let server = Server::new(&cfg).await;
    server.listen().await;
    Ok(())
}
