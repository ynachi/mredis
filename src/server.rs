use crate::config::Config;
use crate::db::Storage;
use crate::parser::Parser;
use std::process;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{debug, error, info};

// @TODO: implement Tracing
// @TODO: implement Metrics
// @TODO: Implement graceful shutdown
// @TODO: Implement Semaphore

pub struct Server {
    storage: Arc<Storage>,
    tcp_listener: TcpListener,
    net_buffer_size: usize,
    conn_limit: Arc<Semaphore>,
}

impl Server {
    pub async fn new(cfg: &Config) -> Self {
        let tcp_listener = match TcpListener::bind((cfg.ip_addr.to_owned(), cfg.port)).await {
            Ok(tcp_listener) => tcp_listener,
            Err(e) => {
                error!("failed to start the TCP server: {}", e);
                process::exit(1);
            }
        };
        let storage = Arc::new(Storage::new(cfg.capacity, cfg.shard_count));
        let conn_limit = Arc::new(Semaphore::new(cfg.max_conn));
        info!("Starting mredis server: {:?}", cfg);
        Server {
            storage,
            tcp_listener,
            net_buffer_size: cfg.network_buffer_size,
            conn_limit,
        }
    }

    pub async fn listen(&self) {
        debug!("server start listening for new connections");
        loop {
            // Check if there is room to get a new connection before
            // We can unwrap because there is only one way this can fail:
            // the semaphore has been
            // closed.
            // And such a case is a programming error, so the program cannot continue.
            // Acquire_owned is used so that we can move the semaphore lock in the tokio task.
            let permit = self
                .conn_limit
                .clone()
                .acquire_owned()
                .await
                .expect("Failed to acquire a permit from the semaphore");

            let conn_string = self.tcp_listener.accept().await;

            match conn_string {
                Ok((stream, addr)) => {
                    debug!("new connection established: {}", addr);

                    let state = self.storage.clone();
                    let mut parser = Parser::new(stream, state, self.net_buffer_size);

                    tokio::spawn(async move {
                        debug!("server initiated a new session");
                        parser.process_frames().await;
                        // we no longer need the connection at this point, so drop it before
                        // we release the semaphore.
                        drop(parser);
                        // release the semaphore
                        drop(permit);
                    });
                }
                Err(err) => {
                    debug!("error accepting client connection: {:?}", err);
                }
            }
        }
    }
}
