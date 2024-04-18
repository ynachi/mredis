use crate::command::Command;
use crate::db::Storage;
use crate::frame::{decode, Frame, FrameError};
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error};

pub struct Config {
    pub ip_addr: String,
    pub port: u16,
    pub capacity: usize,
    pub shard_count: usize,
}

pub struct Server {
    storage: Arc<Storage>,
    tcp_listener: TcpListener,
}

impl Server {
    pub async fn new(cfg: &Config) -> Self {
        let tcp_listener = TcpListener::bind((cfg.ip_addr.to_owned(), cfg.port))
            .await
            .expect("failed to start TCP server");
        let storage = Arc::new(Storage::new(cfg.capacity, cfg.shard_count));

        Server {
            storage,
            tcp_listener,
        }
    }

    pub async fn listen(&self) {
        loop {
            let conn_string = self.tcp_listener.accept().await;
            match conn_string {
                Ok((mut stream, addr)) => {
                    debug!("new connection established: {}", addr);

                    let state = self.storage.clone();

                    tokio::spawn(async move { Self::process_stream(&mut stream, state).await });
                }
                Err(err) => {
                    debug!("error accepting client connection: {:?}", err);
                }
            }
        }
    }

    async fn process_stream(stream: &mut TcpStream, state: Arc<Storage>) {
        let (reader_half, writer_half) = stream.split();

        let mut reader = BufReader::with_capacity(8 * 1024, reader_half);
        let mut writer = BufWriter::with_capacity(8 * 1024, writer_half);

        loop {
            let frame = decode(&mut reader).await;
            match frame {
                Ok(frame) => {
                    debug!("command frame received!");
                    process_frame(frame, &state, &mut writer).await;
                }
                Err(err) => {
                    if seen_eof(&err, &mut writer).await {
                        debug!("client gracefully closed connection");
                        return;
                    }
                }
            }
        }
    }
}

async fn process_frame<T>(frame: Frame, state: &Arc<Storage>, stream_writer: &mut BufWriter<T>)
where
    T: AsyncWriteExt + Unpin,
{
    match Command::from_frame(frame) {
        Ok(cmd) => {
            apply_command(&cmd, state, stream_writer).await;
        }
        Err(err) => send_error(&err, stream_writer).await,
    }
}

async fn apply_command<T: AsyncWriteExt + Unpin>(command: &Command, state: &Arc<Storage>, stream_writer: &mut BufWriter<T>)
{
    let response_frame = match command {
        Command::Ping(message) => {
            if let Some(message) = message {
                Frame::new_bulk_string(message.to_string())
            } else {
                Frame::new_simple_string("PONG".to_string())
            }
        }
        Command::Get(key) => {
            if let Some(ans) = state.get_v(key) {
                Frame::new_simple_string(ans)
            } else {
                Frame::new_null()
            }
        }
        Command::Set(key, value, ttl) => {
            // if let Some(ans) = state.set_kv(key, value, *ttl) {
            //     Frame::new_simple_string(ans)
            // } else {
            //     Frame::new_null()
            // }
            state.set_kv(key, value, *ttl);
            Frame::new_simple_string("OK".to_string())
        }
        Command::Del(frames) => {
            let num_deleted = state.del_entries(frames);
            Frame::new_integer(num_deleted as i64)
        }
        Command::Unknown(name) => Frame::new_bulk_error(format!("unknown command: {}", name)),
    };
    response_frame.write_flush_all(stream_writer).await.unwrap()
}

/// seen_eof filters FrameError because some errors need to be sent back to the client via
/// the network, for instance, syntax errors. In this case, send the error to the client. If EOF
/// return true to the caller. And, only log over error variants.
async fn seen_eof<T: AsyncWriteExt + Unpin>(err: &FrameError, stream_writer: &mut BufWriter<T>) -> bool
{
    match err {
        FrameError::Eof => true,
        FrameError::Incomplete
        | FrameError::Invalid
        | FrameError::Unknown
        | FrameError::UTF8ToInt
        | FrameError::Syntax(_) => {
            send_error(err, stream_writer).await;
            false
        }
        _ => {
            error!("error while decoding frame: {}", err);
            false
        }
    }
}

/// send_error is a wrapper to send errors to the client over the network.
/// These are mostly syntax errors.
async fn send_error<T: AsyncWriteExt + Unpin>(err: &FrameError, stream: &mut BufWriter<T>)
{
    let err_frame = Frame::new_bulk_error(err.to_string());
    if let Err(err) = err_frame.write_flush_all(stream).await {
        error!("failed to write to network: {}", err);
    }
}
