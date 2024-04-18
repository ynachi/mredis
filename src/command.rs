use std::time::Duration;

use crate::frame::{Frame, FrameError};

#[derive(Debug)]
pub enum Command {
    Ping(Option<String>),
    Set(String, String, Duration),
    Get(String),
    Del(Vec<String>),
    Unknown(String),
}

impl Command {
    /// Parse a command from a frame of array of bulk. This method should not be passed
    /// another type or frame contents. Because RESP commands are represented as an array of bulk frames.
    pub fn from_frame(frame: Frame) -> Result<Self, FrameError> {
        // users are aware they should only give an array of bulk frame to this method but we have to
        // check anyway. We opted to perform lazy validation as validating the whole array upfront
        // can be expensive.
        let frames = frame.get_array().ok_or(FrameError::Syntax(
            "commands can only be frame array".to_string(),
        ))?;
        if let Some(name) = Self::get_name(frames) {
            return match name.to_ascii_uppercase().as_str() {
                "PING" => Command::parse_ping(&frames[1..]),
                "GET" => Command::parse_get(&frames[1..]),
                "SET" => Command::parse_set(&frames[1..]),
                "DEL" => Command::parse_del(&frames[1..]),
                _ => Ok(Command::Unknown(name.to_string())),
            };
        }
        Err(FrameError::Syntax(
            "RESP command name should be of type bulk frame".to_string(),
        ))
    }

    /// parse_get tries to retrieve get args from a slice of frames
    fn parse_get(frames: &[Frame]) -> Result<Command, FrameError> {
        if frames.len() != 1 {
            return Err(FrameError::Syntax(
                "PING command takes at most 1 argument".to_string(),
            ));
        }
        Ok(Command::Get(Self::get_string(frames, 0)?))
    }

    fn get_name(frames: &[Frame]) -> Option<&String> {
        if frames.is_empty() {
            return None;
        }
        if let Some((_, name)) = frames[0].get_bulk() {
            return Some(name);
        }
        None
    }

    /// parse_ping tries to retrieve ping args from a slice of frames
    fn parse_ping(frames: &[Frame]) -> Result<Command, FrameError> {
        match frames.len() {
            0 => Ok(Command::Ping(None)),
            1 => Ok(Command::Ping(Some(Self::get_string(frames, 0)?))),
            _ => Err(FrameError::Syntax(
                "PING command takes at most 1 argument".to_string(),
            )),
        }
    }

    fn get_string(frames: &[Frame], index: usize) -> Result<String, FrameError> {
        match frames[index].get_bulk() {
            Some(message) => Ok(message.1.to_owned()),
            None => Err(FrameError::Syntax(
                "RESP args should be of type bulk frame".to_string(),
            )),
        }
    }

    fn parse_set(frames: &[Frame]) -> Result<Command, FrameError> {
        let len = frames.len();
        if len != 2 && len != 4 {
            return Err(FrameError::Syntax(
                "SET should take 2 or 4 arguments".to_string(),
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

        Ok(Command::Set(key, value, ttl))
    }

    fn parse_del(frames: &[Frame]) -> Result<Command, FrameError> {
        let len = frames.len();
        if len == 0 {
            return Err(FrameError::Syntax(
                "DEL command takes at least one key".to_string(),
            ));
        }
        let mut vec = Vec::with_capacity(len);
        for i in 0..len {
            vec.push(Self::get_string(frames, i)?);
        }
        Ok(Command::Del(vec))
    }
    
    fn get_u64(frames: &[Frame], index: usize) -> Result<u64, FrameError> {
        let content = Self::get_string(frames, index)?;
        let content = content.parse::<u64>().map_err(|_| FrameError::UTF8ToInt)?;
        Ok(content)
    }
}
