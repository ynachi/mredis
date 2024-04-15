use std::io::{BufWriter, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{io, thread};
use tracing::debug;

use crate::db;
use crate::frame::Frame;

#[derive(Debug)]
enum Cmd {
    Ping(Option<String>),
    Get(String),
    Set(String, String, Duration),
    Unknown,
}

impl Cmd {
    /// Parse a command from a frame of array of bulk. This method should not be passed
    /// another type or frame contents. Because RESP commands are represented as an array of bulk frames.
    pub fn from_frame(frame: Frame) -> io::Result<Cmd> {
        // users are aware they should only give an array of bulk frame to this method but we have to
        // check anyway. We opted to perform lazy validation as validating the whole array upfront
        // can be expensive.
        let frames = frame.as_array()?;
        if let Some(name) = Self::get_name(frames) {
            return match name.to_ascii_uppercase().as_str() {
                "PING" => Cmd::parse_ping(&frames[1..]),
                "GET" => Cmd::parse_get(&frames[1..]),
                "SET" => Cmd::parse_set(&frames[1..]),
                _ => Ok(Cmd::Unknown),
            };
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RESP command name should be of type bulk frame",
        ))
    }

    fn get_name(frames: &[Frame]) -> Option<&String> {
        if frames.is_empty() {
            return None;
        }
        frames[0].bulk_as_string()
    }
    /// parse_ping tries to retrieve ping args from a slice of frames
    fn parse_ping(frames: &[Frame]) -> io::Result<Cmd> {
        match frames.len() {
            0 => Ok(Cmd::Ping(None)),
            1 => Ok(Cmd::Ping(Some(Self::get_string(frames, 0)?))),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PING command takes at most 1 argument",
            )),
        }
    }

    /// parse_get tries to retrieve get args from a slice of frames
    fn parse_get(frames: &[Frame]) -> io::Result<Cmd> {
        if frames.len() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PING command takes at most 1 argument",
            ));
        }
        Ok(Cmd::Get(Self::get_string(frames, 0)?))
    }

    fn parse_set(frames: &[Frame]) -> io::Result<Cmd> {
        let len = frames.len();
        if len != 2 && len != 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SET should take 2 or 4 arguments",
            ));
        }
        let key = Self::get_string(frames, 0)?;
        let value = Self::get_string(frames, 1)?;

        let mut ttl = Duration::from_millis(0);

        // check if we've got the right option to set the time in millis
        if len > 2 && Self::get_string(frames, 2)?.to_uppercase() == "PX" {
            let expiration = Self::get_u64(frames, 3)?;
            ttl = Duration::from_millis(expiration);
        }

        Ok(Cmd::Set(key, value, ttl))
    }

    fn get_string(frames: &[Frame], index: usize) -> io::Result<String> {
        match frames[index].bulk_as_string() {
            Some(message) => Ok(message.to_owned()),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RESP args should be of type bulk frame",
            )),
        }
    }

    fn get_u64(frames: &[Frame], index: usize) -> io::Result<u64> {
        let content = Self::get_string(frames, index)?;
        let content = content
            .parse::<u64>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid expiration"))?;
        Ok(content)
    }
}
pub struct CmdHandler {
    cmd_handler: JoinHandle<()>,
}

impl CmdHandler {
    fn new(receiver: Receiver<Cmd>, stream: BufWriter<TcpStream>) -> Self {
        let mut state = db::Shard::default();
        // Workers are created at application initialization.
        // Creating them all is a precondition, so failing to do so
        // is a fatal error.
        let cmd_handler = thread::Builder::new()
            .spawn(move || Self::process_cmds(&mut state, receiver, stream))
            .expect("failed to create thread");
        Self { cmd_handler }
    }

    fn process_cmds(db: &mut db::Shard, receiver: Receiver<Cmd>, mut stream: BufWriter<TcpStream>) {
        while let Ok(cmd) = receiver.recv() {}
        debug!("command receiver closed connection");
        write!(stream, "test");
    }
}
