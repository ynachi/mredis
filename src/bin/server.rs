use mredis::server::{Config, Server};

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    // console_subscriber::init();
    let cfg = Config {
        ip_addr: "127.0.0.1".to_string(),
        port: 6379,
        capacity: 1_000_000,
        shard_count: 32,
        network_buffer_size: 8*1024,
    };
    tracing_subscriber::fmt::try_init().expect("unable to initialize logging");
    let server = Server::new(&cfg).await;
    server.listen().await;
    Ok(())
}
