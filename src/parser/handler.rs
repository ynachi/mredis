use crate::db::Storage;
use crate::parser::{Command, CommandType, Frame, FrameData, FrameID};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufStream, ErrorKind};
use tracing::{debug, error, warn};

pub struct Parser<T>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    buf_stream: BufStream<T>,
    storage: Arc<Storage>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum DecodeError {
    // Need more data to decode frame
    Incomplete,
    // Frame is not correctly formatted
    Invalid,
    // Empty buffer should not be passed to get frame from, so this is an error.
    // EmptyBuffer,
    // reached expected EOF
    Eof,
    // Connection unexpectedly reset
    ConnectionReset,
    // Unidentified IO error
    IOError,
    // UTF8 to Int error
    UTF8ToInt,
    // Unknown frame type
    UnknownFrame,
    // This is a programming error. It should not happen.
    Syntax(String),
    // Fatal network error, the network can no longer process traffic
    FatalNetworkError,
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Incomplete => write!(f, "not enough data to decode a full frame"),
            DecodeError::Invalid => write!(f, "frame is not correctly formatted"),
            DecodeError::Eof => write!(f, "seen EOF, this is generally a graceful disconnection"),
            DecodeError::ConnectionReset => write!(f, "unexpected connection reset"),
            // this error should not happen in practice
            DecodeError::IOError => write!(f, "unexpected IO error"),
            DecodeError::UTF8ToInt => write!(f, "utf8 to int decoding error"),
            DecodeError::UnknownFrame => write!(f, "unable to identify the frame type"),
            DecodeError::Syntax(message) => write!(f, "{}", message),
            DecodeError::FatalNetworkError => write!(f, "fatal network error occurred"),
        }
    }
}

// Convert io::Error to DecodeError. Decode error is more specific to what can happen during an
// attempt to decode a frame. Some of the issues can be IO and some other issues like Atoi or
// syntax.
impl From<io::Error> for DecodeError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            ErrorKind::UnexpectedEof => DecodeError::Eof,
            ErrorKind::ConnectionReset => DecodeError::ConnectionReset,
            // @TODO: maybe distinguish between fatal IO error and non fatal ones
            _ => DecodeError::IOError,
        }
    }
}

impl<T> Parser<T>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    pub async fn write_frame(&mut self, frame: &Frame) -> io::Result<()> {
        self.buf_stream
            .write_all(frame.to_string().as_bytes())
            .await?;
        self.buf_stream.flush().await
    }

    pub fn new(stream: T, storage: Arc<Storage>, buffer_size: usize) -> Self {
        debug!("created a new parser instance");
        Self {
            buf_stream: BufStream::with_capacity(buffer_size, buffer_size, stream),
            storage,
        }
    }

    pub async fn decode_frame(&mut self) -> Result<Frame, DecodeError> {
        {
            debug!("started to debug a frame");
            let id = self.get_frame_id().await?;
            match id {
                FrameID::SimpleString
                | FrameID::SimpleError
                | FrameID::Null
                | FrameID::Boolean
                | FrameID::BigNumber
                | FrameID::Integer => self.decode_simple_frame(id).await,

                FrameID::BulkString | FrameID::BulkError => self.decode_bulk_frame(id).await,

                FrameID::Array => {
                    let frame_vec = self.decode_aggregate_frame(id).await?;
                    Ok(Frame {
                        frame_type: FrameID::Array,
                        frame_data: FrameData::Nested(frame_vec),
                    })
                }
            }
        }
    }

    pub async fn process_frames(&mut self) {
        debug!("starting frames decoding loop");
        loop {
            let frame = self.decode_frame().await;
            match frame {
                Ok(frame) => {
                    debug!("command frame received!");
                    let command = frame.to_command();
                    self.apply_command(&command).await;
                    // do your other stuff
                    // decode cmd
                    // apply it
                }
                Err(err) => match err {
                    DecodeError::FatalNetworkError => {
                        error!("process_frames: fatal network error occurred");
                        return;
                    }
                    DecodeError::Eof => {
                        debug!("client gracefully closed connection");
                        return;
                    }
                    _ => {
                        debug!("non fatal decode error occurred")
                    }
                },
            }
        }
    }

    async fn get_frame_id(&mut self) -> Result<FrameID, DecodeError> {
        let id = self.buf_stream.read_u8().await?;
        FrameID::from_u8(&id).ok_or(DecodeError::UnknownFrame)
    }

    async fn decode_bulk_frame(&mut self, id: FrameID) -> Result<Frame, DecodeError> {
        let data = self.read_bulk_string().await?;
        Ok(Frame {
            frame_type: id,
            frame_data: FrameData::Bulk(data),
        })
    }

    /// `read_bulk_string` return a bulk string and its size
    async fn read_bulk_string(&mut self) -> Result<String, DecodeError>
    where
        T: AsyncReadExt + Unpin,
    {
        // e.g: "6\r\nfoobar\r\n"
        let len = self.read_integer().await?;
        // we have to read len + CRLF
        let len = len as usize + 2;

        let mut buf = vec![0; len];
        let size = self.buf_stream.read_exact(&mut buf).await?;
        // we need to read exact size bytes
        if size != len || size < 2 || buf[size - 2] != b'\r' {
            return Err(DecodeError::Invalid);
        }
        Ok(String::from_utf8_lossy(&buf[0..len - 2]).to_string())
    }

    async fn read_integer(&mut self) -> Result<i64, DecodeError>
    where
        T: AsyncReadExt + Unpin,
    {
        let data = self.read_simple_string().await?;
        let data = data.parse().map_err(|_err| DecodeError::Invalid)?;
        Ok(data)
    }

    async fn decode_simple_frame(&mut self, id: FrameID) -> Result<Frame, DecodeError> {
        let data = self.read_simple_string().await?;
        match id {
            FrameID::Boolean => {
                let bool = Self::validate_bool(&data)?;
                Ok(Frame {
                    frame_type: id,
                    frame_data: FrameData::Boolean(bool),
                })
            }
            FrameID::Integer => {
                let data = data.parse().map_err(|_err| DecodeError::UTF8ToInt)?;
                Ok(Frame {
                    frame_type: id,
                    frame_data: FrameData::Integer(data),
                })
            }
            FrameID::Null => {
                if !data.is_empty() {
                    // nil frame should not contain data
                    return Err(DecodeError::Invalid);
                }
                Ok(Frame {
                    frame_type: id,
                    frame_data: FrameData::Null,
                })
            }
            _ => Ok(Frame {
                frame_type: id,
                frame_data: FrameData::Simple(data),
            }),
        }
    }

    fn validate_bool(data: &str) -> Result<bool, DecodeError> {
        match data {
            "t" => Ok(true),
            "f" => Ok(false),
            _ => Err(DecodeError::Invalid),
        }
    }

    /// `read_simple_string` gets a simple string from the network. As a reminder, such string does
    /// not contain any CR or LF char in the middle. This method assumes the frame identifier has
    /// already been taken from the stream. So, for instance, consider you have something like
    /// `HELLO\r\n` instead of `+HELLO\r\n` in the stream while calling this method.
    /// The error returned is the same as `tokio::io::BufReader::read_until()` or one of the following:
    async fn read_simple_string(&mut self) -> Result<String, DecodeError>
    where
        T: AsyncReadExt + Unpin,
    {
        let mut buf = Vec::new();
        let size = self.buf_stream.read_until(b'\n', &mut buf).await?;
        match size {
            0 => Err(DecodeError::Eof),
            _ => {
                if size < 2 {
                    return Err(DecodeError::Incomplete);
                }
                if buf[size - 1] != b'\n' {
                    return Err(DecodeError::Incomplete);
                }
                if buf[size - 2] != b'\r' {
                    return Err(DecodeError::Invalid);
                }
                // We should also check if there is any CR in the middle, but this check is made upfront.
                // The reason is to perform this expensive check only if needed. Also, this function result
                // is used in places that naturally check the correctness of the frame content (for instance, conversion to int).
                Ok(String::from_utf8_lossy(&buf[0..size - 2]).to_string())
            }
        }
    }

    /// decode_aggregate_frame decodes a bucket of frames iteratively.
    /// We have frame ID in the signature because aggregate can be of different types.
    /// So, we need to keep track of the IDs to construct the right aggregate frame when needed.
    /// This function can be used to decode Arrays, Maps, and Sets.
    async fn decode_aggregate_frame(&mut self, id: FrameID) -> Result<Vec<Frame>, DecodeError> {
        // "3\r\n:1\r\n:2\r\n:3\r\n" -> [1, 2, 3]
        // "*2\r\n:1\r\n*1\r\n+Three\r\n"
        let count = self.read_integer().await?;
        let frames: Vec<Frame> = Vec::new();
        let mut stack = Vec::new();
        stack.push((id, count, frames));
        loop {
            let id = self.get_frame_id().await?;
            match id {
                FrameID::Array => {
                    let count = self.read_integer().await?;
                    let frames: Vec<Frame> = Vec::new();
                    stack.push((id, count, frames));
                }
                _ => {
                    // we have a non-aggregate frame, and there is nothing in the stack it can be appended
                    // to so this is an error.
                    if stack.is_empty() {
                        return Err(DecodeError::Invalid);
                    }
                    let frame = self.process_non_aggregate(id).await?;
                    let (_, count, frames) = stack.last_mut().unwrap();
                    frames.push(frame);
                    *count -= 1;
                    // If count == 0, we've decoded an entire array. So push it to the penultimate
                    // aggregate in the stack if any. If there is no more array in the stack, this means
                    // we should return as the total frame was completely processed.
                    if *count == 0 {
                        // We need to loop to successively pop completed vector of frames and push
                        // them to their parent
                        // until we finish piping or find a vector which is incomplete.
                        loop {
                            let (_, _, last_vec_of_frames) = stack.pop().unwrap();
                            // The full global frame was decoded, so return
                            if stack.is_empty() {
                                return Ok(last_vec_of_frames);
                            }
                            // we fully decoded an aggregate but not the full global frame
                            let (id, count, frames) = stack.last_mut().unwrap();
                            // Here is why we needed to keep track of the IDs,
                            // to build the right aggregate.
                            frames.push(Frame {
                                frame_type: *id,
                                frame_data: FrameData::Nested(last_vec_of_frames),
                            });
                            *count -= 1;
                            if *count != 0 {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    /// process_non_aggregate is a helper to decode non-aggregate frames. It calls the appropriate
    /// processing method depending on the frame type. It should not receive an aggregate type.
    pub async fn process_non_aggregate(&mut self, id: FrameID) -> Result<Frame, DecodeError> {
        match id {
            FrameID::Array => Err(DecodeError::Syntax(
                "received aggregate frame in non aggregate decoding".to_string(),
            )),
            FrameID::BulkString | FrameID::BulkError => self.decode_bulk_frame(id).await,
            _ => self.decode_simple_frame(id).await,
        }
    }

    async fn apply_command(&mut self, command: &Command) {
        match command.command_type {
            CommandType::PING => {
                self.apply_ping_command(command).await;
            }
            CommandType::ERROR => {
                self.apply_error_command(command).await;
            }
            _ => unimplemented!(),
        }
    }

    async fn apply_ping_command(&mut self, command: &Command) {
        warn!("receive ping command, processing it");
        let mut response_frame = Frame {
            frame_type: FrameID::Integer,
            frame_data: FrameData::Simple(String::from("PONG")),
        };
        if command.args.len() == 2 {
            response_frame.frame_type = FrameID::BulkString;
            response_frame.frame_data = FrameData::Bulk(command.args[1].clone());
        }
        if let Err(err) = self.write_frame(&response_frame).await {
            error!("failed to write to network: {}", err);
        }
    }

    async fn apply_error_command(&mut self, command: &Command) {
        warn!("receive error command, processing it");
        let response_frame = Frame {
            frame_type: FrameID::BulkError,
            frame_data: FrameData::Bulk(command.args[1].clone()),
        };
        if let Err(err) = self.write_frame(&response_frame).await {
            error!("failed to write to network: {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_decode_frame_integer() {
        let (mut client, server) = io::duplex(1024);
        let storage = Arc::new(Storage::new(1000000, 4));
        let mut parser = Parser::new(server, storage, 1024);

        // Simulate client writing to the stream
        tokio::spawn(async move {
            let data = b":33\r\n:0\r\n:-50\r\n:hello\r\n";
            client.write_all(data).await.unwrap();
            client.flush().await.unwrap();
        });

        // simple string
        let frame = parser.decode_frame().await.unwrap();
        let mut response_frame = Frame::new_integer(33);
        assert_eq!(frame, response_frame, "can decode a positive number");

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_integer(0);
        assert_eq!(
            frame, response_frame,
            "can decode 0 as a number"
        );

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_integer(-50);
        assert_eq!(
            frame, response_frame,
            "can decode a negative number"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::UTF8ToInt),
            "cannot convert an non-number  frame to a number"
        );
    }
    
    #[tokio::test]
    async fn test_decode_frame_simple_string() {
        let (mut client, server) = io::duplex(1024);
        let storage = Arc::new(Storage::new(1000000, 4));
        let mut parser = Parser::new(server, storage, 1024);

        // Simulate client writing to the stream
        tokio::spawn(async move {
            let data = b"+hello\r\n+58\r\n+\r\n+hello\n+Incompet";
            client.write_all(data).await.unwrap();
            client.flush().await.unwrap();
        });

        // simple string
        let frame = parser.decode_frame().await.unwrap();
        let mut response_frame = Frame::new_simple_string("hello");
        assert_eq!(frame, response_frame, "can decode a simple string");

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_simple_string("58");
        assert_eq!(
            frame, response_frame,
            "can decode a simple string which is a number"
        );

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_simple_string("");
        assert_eq!(
            frame, response_frame,
            "can decode a simple string which is empty"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Invalid),
            "simple frame cannot be terminated with a single LF"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Incomplete),
            "frames are terminated with CRLF"
        );
    }

    #[tokio::test]
    async fn test_decode_frame_simple_error() {
        let (mut client, server) = io::duplex(1024);
        let storage = Arc::new(Storage::new(1000000, 4));
        let mut parser = Parser::new(server, storage, 1024);

        // Simulate client writing to the stream
        tokio::spawn(async move {
            let data = b"-hello\r\n-58\r\n-\r\n-hello\n-Incompet";
            client.write_all(data).await.unwrap();
            client.flush().await.unwrap();
        });

        // simple string
        let frame = parser.decode_frame().await.unwrap();
        let mut response_frame = Frame::new_simple_error("hello");
        assert_eq!(frame, response_frame, "can decode a simple error");

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_simple_error("58");
        assert_eq!(
            frame, response_frame,
            "can decode a simple error which is a number"
        );

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_simple_error("");
        assert_eq!(
            frame, response_frame,
            "can decode a simple error which is empty"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Invalid),
            "simple frame cannot be terminated with a single LF"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Incomplete),
            "frames are terminated with CRLF"
        );
    }

    #[tokio::test]
    async fn test_decode_frame_bulk_string() {
        let (mut client, server) = io::duplex(1024);
        let storage = Arc::new(Storage::new(1000000, 4));
        let mut parser = Parser::new(server, storage, 1024);

        // Simulate client writing to the stream
        tokio::spawn(async move {
            let data = b"$5\r\nhello\r\n$6\r\nhel\rlo\r\n$6\r\nhel\nlo\r\n$6\r\nhellojj\r";
            client.write_all(data).await.unwrap();
            client.flush().await.unwrap();
        });

        // simple string
        let frame = parser.decode_frame().await.unwrap();
        let mut response_frame = Frame::new_bulk_string("hello");
        assert_eq!(frame, response_frame, "can decode a bulk string");

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_bulk_string("hel\rlo");
        assert_eq!(
            frame, response_frame,
            "bulk frame can contain CR in the middle"
        );

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_bulk_string("hel\nlo");
        assert_eq!(
            frame, response_frame,
            "bulk frame can contain LF in the middle"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Invalid),
            "bulk string is terminated by CRLF"
        );
    }

    #[tokio::test]
    async fn test_decode_frame_bulk_error() {
        let (mut client, server) = io::duplex(1024);
        let storage = Arc::new(Storage::new(1000000, 4));
        let mut parser = Parser::new(server, storage, 1024);

        // Simulate client writing to the stream
        tokio::spawn(async move {
            let data = b"!5\r\nhello\r\n!6\r\nhel\rlo\r\n!6\r\nhel\nlo\r\n!6\r\nhellojj\r";
            client.write_all(data).await.unwrap();
            client.flush().await.unwrap();
        });

        // simple string
        let frame = parser.decode_frame().await.unwrap();
        let mut response_frame = Frame::new_bulk_error("hello");
        assert_eq!(frame, response_frame, "can decode a bulk string");

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_bulk_error("hel\rlo");
        assert_eq!(
            frame, response_frame,
            "bulk frame can contain CR in the middle"
        );

        let frame = parser.decode_frame().await.unwrap();
        response_frame = Frame::new_bulk_error("hel\nlo");
        assert_eq!(
            frame, response_frame,
            "bulk frame can contain LF in the middle"
        );

        let frame = parser.decode_frame().await;
        assert_eq!(
            frame,
            Err(DecodeError::Invalid),
            "bulk string is terminated by CRLF"
        );
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[tokio::test]
//     async fn decode_test() {
//         //
//         // good simple string + decode from the same frame
//         //
//         let mut stream = BufReader::new("+OK\r\n+\r\n-err\n".as_bytes());
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::SimpleString);
//         assert_eq!(frame.frame_data, FrameData::Simple("OK".to_string()));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::SimpleString);
//         assert_eq!(frame.frame_data, FrameData::Simple("".to_string()));
//
//         // wrongly terminated frame
//         let frame = decode(&mut stream).await;
//         assert_eq!(
//             frame,
//             Err(FrameError::Invalid),
//             "There is no CRLF at the end"
//         );
//
//         let frame = decode(&mut stream).await;
//         assert_eq!(
//             frame,
//             Err(FrameError::Eof),
//             "should return EOF error variant"
//         );
//
//         //
//         // Bulk + err + int + bool
//         //
//         let mut stream = BufReader::new(
//             "$5\r\nhello\r\n-err\r\n:66\r\n:-5\r\n:0\r\n#t\r\n#f\r\n#n\r\n".as_bytes(),
//         );
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::BulkString);
//         assert_eq!(frame.frame_data, FrameData::Bulk(5, "hello".to_string()));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::SimpleError);
//         assert_eq!(frame.frame_data, FrameData::Simple("err".to_string()));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Integer);
//         assert_eq!(frame.frame_data, FrameData::Integer(66));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Integer);
//         assert_eq!(frame.frame_data, FrameData::Integer(-5));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Integer);
//         assert_eq!(frame.frame_data, FrameData::Integer(0));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Boolean);
//         assert_eq!(frame.frame_data, FrameData::Boolean(true));
//
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Boolean);
//         assert_eq!(frame.frame_data, FrameData::Boolean(false));
//
//         let frame = decode(&mut stream).await;
//         assert_eq!(frame, Err(FrameError::Invalid), "invalid bool payload");
//
//         //
//         //Array
//         //
//         let mut stream = BufReader::new("*3\r\n:1\r\n+Two\r\n$5\r\nThree\r\n".as_bytes());
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Array);
//         let frame_data = FrameData::Nested(vec![
//             Frame {
//                 frame_type: FrameID::Integer,
//                 frame_data: FrameData::Integer(1),
//             },
//             Frame {
//                 frame_type: FrameID::SimpleString,
//                 frame_data: FrameData::Simple("Two".to_string()),
//             },
//             Frame {
//                 frame_type: FrameID::BulkString,
//                 frame_data: FrameData::Bulk(5, "Three".to_string()),
//             },
//         ]);
//         assert_eq!(frame.frame_data, frame_data);
//
//         // nested 1
//         let mut stream = BufReader::new("*2\r\n:1\r\n*1\r\n+Three\r\n".as_bytes());
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Array);
//         let frame_data = FrameData::Nested(vec![
//             Frame {
//                 frame_type: FrameID::Integer,
//                 frame_data: FrameData::Integer(1),
//             },
//             Frame {
//                 frame_type: FrameID::Array,
//                 frame_data: FrameData::Nested(vec![Frame {
//                     frame_type: FrameID::SimpleString,
//                     frame_data: FrameData::Simple("Three".to_string()),
//                 }]),
//             },
//         ]);
//         assert_eq!(frame.frame_data, frame_data);
//
//         // nested 2
//         let mut stream = BufReader::new("*3\r\n:1\r\n*1\r\n+Three\r\n-Err\r\n".as_bytes());
//         let frame = decode(&mut stream).await.unwrap();
//         assert_eq!(frame.frame_type, FrameID::Array);
//         let frame_data = FrameData::Nested(vec![
//             Frame {
//                 frame_type: FrameID::Integer,
//                 frame_data: FrameData::Integer(1),
//             },
//             Frame {
//                 frame_type: FrameID::Array,
//                 frame_data: FrameData::Nested(vec![Frame {
//                     frame_type: FrameID::SimpleString,
//                     frame_data: FrameData::Simple("Three".to_string()),
//                 }]),
//             },
//             Frame {
//                 frame_type: FrameID::SimpleError,
//                 frame_data: FrameData::Simple("Err".to_string()),
//             },
//         ]);
//         assert_eq!(frame.frame_data, frame_data);
//     }
// }
