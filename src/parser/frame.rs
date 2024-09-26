use crate::parser::{Command, CommandType};
use std::fmt;
use std::fmt::{Display, Formatter};
use tokio::io::{self, AsyncWriteExt, BufWriter};
use tracing::debug;

/// `FrameID` is used to mark the beginning of a frame type. We have decided to implement only what
/// is needed as we go. This is why there are some commented types. We wanted to implement them all
///  up front, but we have changed our mind.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FrameID {
    Integer = 58, // ':'
    // @TODO: remove for now
    // Double = 44,       // ','
    SimpleString = 43, // '+'
    SimpleError = 45,  // '-'
    BulkString = 36,   // '$'
    BulkError = 33,    // '!'
    // @TODO: remove for now
    // VerbatimString = 61, // '='
    Boolean = 35,   // '#'
    Null = 95,      // '_'
    BigNumber = 40, // '('
    Array = 42,     // '*'
                    // @TODO: remove for now
                    // Map = 37,       // '%'
                    // Set = 126,      // '~'
                    // Push = 62,      // '>'
}

impl FrameID {
    pub(crate) fn from_u8(from: &u8) -> Option<FrameID> {
        match from {
            58 => Some(FrameID::Integer),
            // 44 => Some(FrameID::Double),
            43 => Some(FrameID::SimpleString),
            45 => Some(FrameID::SimpleError),
            36 => Some(FrameID::BulkString),
            33 => Some(FrameID::BulkError),
            35 => Some(FrameID::Boolean),
            95 => Some(FrameID::Null),
            40 => Some(FrameID::BigNumber),
            42 => Some(FrameID::Array),
            // 37 => Some(FrameID::Map),
            // 126 => Some(FrameID::Set),
            // 62 => Some(FrameID::Push),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FrameData {
    Null,
    Simple(String),
    Integer(i64),
    Boolean(bool),
    Bulk(String),
    Nested(Vec<Frame>),
}

impl FrameData {
    fn get_integer(&self) -> Option<i64> {
        match self {
            FrameData::Integer(value) => Some(*value),
            _ => None,
        }
    }
    fn get_string(&self) -> Option<&String> {
        match self {
            FrameData::Simple(value) => Some(value),
            _ => None,
        }
    }
    fn get_bulk(&self) -> Option<&String> {
        match self {
            FrameData::Bulk(data) => Some(data),
            _ => None,
        }
    }
    fn get_boolean(&self) -> Option<bool> {
        match self {
            FrameData::Boolean(value) => Some(*value),
            _ => None,
        }
    }
    pub fn get_nested(&self) -> Option<&Vec<Frame>> {
        match self {
            FrameData::Nested(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    pub(crate) frame_type: FrameID,
    pub(crate) frame_data: FrameData,
}

impl Frame {
    pub fn get_array(&self) -> Option<&Vec<Frame>> {
        if self.frame_type != FrameID::Array {
            return None;
        }
        self.frame_data.get_nested()
    }
    pub fn get_bulk(&self) -> Option<&String> {
        match &self.frame_data {
            FrameData::Bulk(data) => Some(data),
            _ => None,
        }
    }

    pub async fn write_flush_all<T>(&self, stream: &mut BufWriter<T>) -> io::Result<()>
    where
        T: AsyncWriteExt + Unpin,
    {
        stream.write_all(self.to_string().as_bytes()).await?;
        stream.flush().await
    }

    pub fn new_bulk_error(inner: &str) -> Frame {
        Frame {
            frame_type: FrameID::BulkError,
            frame_data: FrameData::Bulk(inner.to_string()),
        }
    }

    pub fn new_simple_string(inner: &str) -> Frame {
        Frame {
            frame_type: FrameID::SimpleString,
            frame_data: FrameData::Simple(inner.to_string()),
        }
    }

    pub fn new_bulk_string(inner: &str) -> Frame {
        Frame {
            frame_type: FrameID::BulkString,
            frame_data: FrameData::Bulk(inner.to_string()),
        }
    }

    pub fn new_null() -> Frame {
        Frame {
            frame_type: FrameID::Null,
            frame_data: FrameData::Null,
        }
    }

    pub fn new_integer(inner: i64) -> Frame {
        Frame {
            frame_type: FrameID::Integer,
            frame_data: FrameData::Integer(inner),
        }
    }

    pub fn new_simple_error(inner: &str) -> Frame {
        Frame {
            frame_type: FrameID::SimpleError,
            frame_data: FrameData::Simple(inner.to_string()),
        }
    }

    pub(crate) fn to_command(&self) -> Command {
        // If self.validate_command_array() returns None, the method continues execution.
        if let Some(command) = self.validate_command_array() {
            return command;
        }

        // It is safe to unwrap as we validated the frame array just before.
        // This assumes self.get_array() is infallible after self.validate_command_array() is Some.
        let args_frames = self.get_array().unwrap();
        let cmd_name = args_frames[0].get_bulk().unwrap().to_uppercase();

        if let Some(command_type) = Command::make_redis_command_map().get(cmd_name.as_str()) {
            return match command_type {
                CommandType::PING => Command::parse_ping_command(args_frames),
                // err commands are called NO_NAME
                _ => Command {
                    command_type: CommandType::ERROR,
                    args: vec!["NO_NAME".to_string(), "hello".to_string()],
                },
            };
        }

        // Informing that an unknown command was received.
        let msg = format!("unknown command '{cmd_name}'");
        Command::new(CommandType::ERROR, &vec![cmd_name, msg])
    }

    // checks if a Frame can be used to successfully parse a command without actually parsing it.
    // It returns a frame error if it cannot and None when it can.
    fn validate_command_array(&self) -> Option<Command> {
        let err_command_name = "NO_NAME".to_string();
        if self.frame_type != FrameID::Array {
            let msg = "only array can represent a redis command".to_string();
            return Some(Command {
                command_type: CommandType::ERROR,
                args: vec![err_command_name, msg],
            });
        }

        // it is safe to unwrap because at this point, we know it is an array
        let array = self.get_array().unwrap();
        if array.is_empty() {
            let msg = "cannot parse command from empty frame array".to_string();
            return Some(Command {
                command_type: CommandType::ERROR,
                args: vec![err_command_name, msg],
            });
        }

        for frame in array {
            if frame.frame_type != FrameID::BulkString {
                let msg = format!("invalid frame type: {:?}", frame.frame_type);
                return Some(Command {
                    command_type: CommandType::ERROR,
                    args: vec![err_command_name, msg],
                });
            }
        }

        None
    }
}

impl Display for Frame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.frame_type {
            FrameID::Integer => {
                debug!("encoding Integer frame");
                let value = self.frame_data.get_integer().ok_or(fmt::Error)?;
                write!(f, ":{}\r\n", value)
            }
            FrameID::SimpleString => {
                debug!("encoding SimpleString frame");
                let value = self.frame_data.get_string().ok_or(fmt::Error)?;
                write!(f, "+{}\r\n", value)
            }
            FrameID::SimpleError => {
                debug!("encoding SimpleError frame");
                let value = self.frame_data.get_string().ok_or(fmt::Error)?;
                write!(f, "-{}\r\n", value)
            }
            FrameID::BulkString => {
                debug!("encoding BulkString frame");
                let bulk_data = self.frame_data.get_bulk().ok_or(fmt::Error)?;
                write!(f, "${}\r\n{}\r\n", bulk_data.len(), bulk_data)
            }
            FrameID::BulkError => {
                debug!("encoding BulkError frame");
                let bulk_data = self.frame_data.get_bulk().ok_or(fmt::Error)?;
                write!(f, "!{}\r\n{}\r\n", bulk_data.len(), bulk_data)
            }
            FrameID::Boolean => {
                debug!("encoding Boolean frame");
                let value = self.frame_data.get_boolean().ok_or(fmt::Error)?;
                let value = if value { "t" } else { "f" };
                write!(f, "#{}\r\n", value)
            }
            FrameID::Null => {
                debug!("encoding Null frame");
                write!(f, "_\r\n")
            }
            FrameID::BigNumber => {
                debug!("encoding BigNumber frame");
                let value = self.frame_data.get_string().ok_or(fmt::Error)?;
                write!(f, "({}\r\n", value)
            }
            FrameID::Array => {
                debug!("encoding Array frame");
                let frames = self.frame_data.get_nested().ok_or(fmt::Error)?;
                write!(f, "*{}\r\n", frames.len())?;
                for v in frames {
                    write!(f, "{}", v)?;
                }
                Ok(())
            }
        }
    }
}