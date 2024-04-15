//! This crate provides utilities decode/encode a RESP frame from a stream.
//! Some high-level choices have been made. Let's share some contexts.
//!
//! 1. Not all frame types will be implemented in the beginning. We first implement those we know
//! we will be using for sure. The rest will be implemented when the need appears.
//! 2. Quick note about simple frames. Simple frames should not contain any CR or LF in the middle.
//! Vut this check will not be made during the decoding of frames. Instead, we will make sure that
//! simple frames are valid at their creation. We do that because we want to pay the cost of checking
//! this property only if needed as it is expensive.

use std::io;
use std::io::{BufRead, BufReader, Read};
use std::io::{BufWriter, ErrorKind, Write};
use tracing::{debug, error, warn};

const CR: u8 = b'\r';
const LF: u8 = b'\n';

/// `FrameID` is used to mark the beginning of a frame type. We have decided to implement only what
/// is needed as we go. This is why there are some commented types. We wanted to implement them all
///  up front, but we have changed our mind.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
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
    // fn as_u8(&self) -> u8 {
    //     match self {
    //         FrameID::Integer => 58,
    //         // FrameID::Double => 44,
    //         FrameID::SimpleString => 43,
    //         FrameID::SimpleError => 45,
    //         FrameID::BulkString => 36,
    //         FrameID::BulkError => 33,
    //         FrameID::Boolean => 35,
    //         FrameID::Null => 95,
    //         FrameID::BigNumber => 40,
    //         FrameID::Array => 42,
    //         // FrameID::Map => 37,
    //         // FrameID::Set => 126,
    //         // FrameID::Push => 62,
    //     }
    // }

    fn from_u8(from: &u8) -> Option<FrameID> {
        match from {
            58 => Some(FrameID::Integer),
            43 => Some(FrameID::SimpleString),
            45 => Some(FrameID::SimpleError),
            36 => Some(FrameID::BulkString),
            33 => Some(FrameID::BulkError),
            35 => Some(FrameID::Boolean),
            95 => Some(FrameID::Null),
            40 => Some(FrameID::BigNumber),
            42 => Some(FrameID::Array),
            _ => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum FrameData {
    Null,
    Simple(String),
    Integer(i64),
    Boolean(bool),
    Bulk(usize, String),
    Nested(Vec<Frame>),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Frame {
    frame_type: FrameID,
    frame_data: FrameData,
}

impl Frame {
    pub(crate) fn as_array(&self) -> io::Result<&Vec<Frame>> {
        match (self.frame_type, &self.frame_data) {
            (FrameID::Array, FrameData::Nested(vec)) => Ok(&vec),
            _ => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "commands can only be parsed from Arrays",
            )),
        }
    }

    /// bulk_as_string returns the inner string of a Bulk frame type or None.
    pub(crate) fn bulk_as_string(&self) -> Option<&String> {
        match (self.frame_type, &self.frame_data) {
            (FrameID::BulkString, FrameData::Bulk(_, content)) => Some(content),
            _ => None,
        }
    }

    fn write(&self, stream: &mut BufWriter<impl Write>) -> io::Result<()> {
        match (self.frame_type, &self.frame_data) {
            (FrameID::Integer, FrameData::Integer(content)) => {
                write!(stream, ":{}\r\n", content)?;
            }
            (FrameID::SimpleString, FrameData::Simple(content)) => {
                write!(stream, "+{}\r\n", content)?;
            }
            (FrameID::SimpleError, FrameData::Simple(content)) => {
                write!(stream, "-{}\r\n", content)?;
            }
            (FrameID::BulkString, FrameData::Bulk(size, content)) => {
                write!(stream, "${}\r\n{}\r\n", size, content)?;
            }
            (FrameID::BulkError, FrameData::Bulk(size, content)) => {
                write!(stream, "!{}\r\n{}\r\n", size, content)?;
            }
            (FrameID::Boolean, FrameData::Boolean(content)) => {
                let content = if *content { "t" } else { "f" };
                write!(stream, "#{}\r\n", content)?;
            }
            (FrameID::Null, FrameData::Null) => {
                write!(stream, "_\r\n")?;
            }
            (FrameID::BigNumber, FrameData::Simple(content)) => {
                write!(stream, "({}\r\n", content)?;
            }
            (FrameID::Array, FrameData::Nested(content)) => {
                write!(stream, "*{}\r\n", content.len())?;
                for f in content {
                    f.write(stream)?
                }
            }
            // this case can never happen
            _ => unreachable!(),
        }
        stream.flush()
    }
}

fn validate_bool(data: String) -> Result<bool, FrameError> {
    match data.as_str() {
        "t" => Ok(true),
        "f" => Ok(false),
        _ => Err(FrameError::Invalid),
    }
}

fn decode<T>(stream: &mut BufReader<T>) -> Result<Frame, FrameError>
where
    T: Read,
{
    let id = get_frame_id(stream)?;
    match id {
        FrameID::SimpleString
        | FrameID::SimpleError
        | FrameID::Null
        | FrameID::Boolean
        | FrameID::BigNumber
        | FrameID::Integer => process_simple_frames(id, stream),

        FrameID::BulkString | FrameID::BulkError => process_bulk_frames(id, stream),

        FrameID::Array => {
            let frame_vec = process_aggregate_frames(id, stream)?;
            Ok(Frame {
                frame_type: FrameID::Array,
                frame_data: FrameData::Nested(frame_vec),
            })
        }
    }
}

fn process_simple_frames<T>(id: FrameID, stream: &mut BufReader<T>) -> Result<Frame, FrameError>
where
    T: Read,
{
    let data = read_simple_string(stream)?;
    match id {
        FrameID::Boolean => {
            let bool = validate_bool(data)?;
            Ok(Frame {
                frame_type: id,
                frame_data: FrameData::Boolean(bool),
            })
        }
        FrameID::Integer => {
            let data = data.parse().map_err(|_err| FrameError::UTF8ToInt)?;
            Ok(Frame {
                frame_type: id,
                frame_data: FrameData::Integer(data),
            })
        }
        FrameID::Null => {
            if !data.is_empty() {
                // nil frame should not contain data
                return Err(FrameError::Invalid);
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

fn process_bulk_frames<T>(id: FrameID, stream: &mut BufReader<T>) -> Result<Frame, FrameError>
where
    T: Read,
{
    let (size, data) = read_bulk_string(stream)?;
    Ok(Frame {
        frame_type: id,
        frame_data: FrameData::Bulk(size, data),
    })
}

/// process_non_aggregate is a helper to decode non-aggregate frames. It calls the appropriate
/// processing method depending on the frame type. It should not receive an aggregate type.
fn process_non_aggregate<T>(id: FrameID, stream: &mut BufReader<T>) -> Result<Frame, FrameError>
where
    T: Read,
{
    match id {
        FrameID::Array => Err(FrameError::Syntax),
        FrameID::BulkString | FrameID::BulkError => process_bulk_frames(id, stream),
        _ => process_simple_frames(id, stream),
    }
}

/// process_aggregate_frames decodes a bucket of frames iteratively.
/// We have frame ID in the signature because aggregate can be of different types.
/// So, we need to keep track of the IDs to construct the right aggregate frame when needed.
/// This function can be used to decode Arrays, Maps, and Sets.
fn process_aggregate_frames<T>(
    id: FrameID,
    stream: &mut BufReader<T>,
) -> Result<Vec<Frame>, FrameError>
where
    T: Read,
{
    // "3\r\n:1\r\n:2\r\n:3\r\n" -> [1, 2, 3]
    // "*2\r\n:1\r\n*1\r\n+Three\r\n"
    let count = read_integer(stream)?;
    let frames: Vec<Frame> = Vec::new();
    let mut stack = Vec::new();
    stack.push((id, count, frames));
    loop {
        let id = get_frame_id(stream)?;
        match id {
            FrameID::Array => {
                let count = read_integer(stream)?;
                let frames: Vec<Frame> = Vec::new();
                stack.push((id, count, frames));
            }
            _ => {
                // we have a non-aggregate frame, and there is nothing in the stack it can be appended
                // to so this is an error.
                if stack.is_empty() {
                    return Err(FrameError::Invalid);
                }
                let frame = process_non_aggregate(id, stream)?;
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

#[derive(Debug, Eq, PartialEq)]
pub enum FrameError {
    // Need more data to decode frame
    Incomplete,
    // Frame is not correctly formatted
    Invalid,
    // reached expected EOF
    Eof,
    // Connection unexpectedly reset
    ConnectionReset,
    // Unidentified IO error
    IOError,
    // UTF8 to Int error
    UTF8ToInt,
    // Unknown frame type
    Unknown,
    // This is a programming error. It should not happen.
    Syntax,
}

/// `read_simple_string` gets a simple string from the network. As a reminder, such a string does
/// not contain any CR or LF char in the middle. This method assumes the frame identifier has
/// already been taken from the stream. So, for instance, consider you have something like
/// `HELLO\r\n` instead of `+HELLO\r\n` in the stream while calling this method.
/// The error returned is the same as `tokio::io::BufReader::read_until()` or one of the following:
fn read_simple_string<T>(stream: &mut BufReader<T>) -> Result<String, FrameError>
where
    T: Read,
{
    let mut buf = Vec::new();
    match stream.read_until(LF, &mut buf) {
        Ok(0) => Err(FrameError::Eof),
        Ok(size) => {
            if size < 2 {
                return Err(FrameError::Incomplete);
            } else if buf[size - 2] != CR {
                return Err(FrameError::Invalid);
            }
            // We should also check if there is any CR in the middle, but this check is made upfront.
            // The reason is to perform this expensive check only if needed. Also, this function result
            // is used in places that naturally check the correctness of the frame content (for instance, conversion to int).
            Ok(String::from_utf8_lossy(&buf[0..size - 2]).to_string())
        }
        Err(e) if e.kind() == ErrorKind::ConnectionReset => {
            warn!("connection  reset by peer: {}", e);
            Err(FrameError::ConnectionReset)
        }
        Err(e) => {
            error!("error while reading from buffer: {}", e);
            Err(FrameError::IOError)
        }
    }
}

fn get_frame_id<T>(stream: &mut BufReader<T>) -> Result<FrameID, FrameError>
where
    T: Read,
{
    let mut bytes = [0; 1];
    match stream.read(&mut bytes) {
        Ok(0) => Err(FrameError::Eof),
        Ok(_) => match FrameID::from_u8(&bytes[0]) {
            Some(id) => Ok(id),
            None => Err(FrameError::Unknown),
        },
        Err(e) if e.kind() == ErrorKind::ConnectionReset => {
            warn!("connection  reset by peer: {}", e);
            Err(FrameError::ConnectionReset)
        }
        Err(e) => {
            error!("error while reading from buffer: {}", e);
            Err(FrameError::IOError)
        }
    }
}

fn read_integer<T>(stream: &mut BufReader<T>) -> Result<i64, FrameError>
where
    T: Read,
{
    let data = read_simple_string(stream)?;
    let data = data.parse().map_err(|_err| FrameError::Invalid)?;
    Ok(data)
}

/// `read_bulk_string` return a bulk string and its size
fn read_bulk_string<T>(stream: &mut BufReader<T>) -> Result<(usize, String), FrameError>
where
    T: Read,
{
    // e.g: "6\r\nfoobar\r\n"
    let len = read_integer(stream)?;
    // we have to read len + CRLF
    let len = len as usize + 2;

    let mut buf = vec![0; len];
    match stream.read_exact(&mut buf) {
        Ok(_) => {
            // we need to read exact size bytes
            let size = buf.len();
            if size != len || size < 2 || buf[size - 2] != CR {
                return Err(FrameError::Invalid);
            }
            Ok((
                size - 2,
                String::from_utf8_lossy(&buf[0..len - 2]).to_string(),
            ))
        }
        // The caller will treat EOF differently, so it needs to be returned explicitly
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
            debug!("EOF seen before full decode: {}", e);
            Err(FrameError::ConnectionReset)
        }
        Err(e) => {
            error!("error while reading from buffer: {}", e);
            Err(FrameError::IOError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_test() {
        //
        // good simple string + decode from the same frame
        //
        let mut stream = BufReader::new("+OK\r\n+\r\n-err\n".as_bytes());
        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::SimpleString);
        assert_eq!(frame.frame_data, FrameData::Simple("OK".to_string()));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::SimpleString);
        assert_eq!(frame.frame_data, FrameData::Simple("".to_string()));

        // wrongly terminated frame
        let frame = decode(&mut stream);
        assert_eq!(
            frame,
            Err(FrameError::Invalid),
            "There is no CRLF at the end"
        );

        let frame = decode(&mut stream);
        assert_eq!(
            frame,
            Err(FrameError::Eof),
            "should return EOF error variant"
        );

        //
        // Bulk + err + int + bool
        //
        let mut stream = BufReader::new(
            "$5\r\nhello\r\n-err\r\n:66\r\n:-5\r\n:0\r\n#t\r\n#f\r\n#n\r\n".as_bytes(),
        );
        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::BulkString);
        assert_eq!(frame.frame_data, FrameData::Bulk(5, "hello".to_string()));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::SimpleError);
        assert_eq!(frame.frame_data, FrameData::Simple("err".to_string()));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Integer);
        assert_eq!(frame.frame_data, FrameData::Integer(66));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Integer);
        assert_eq!(frame.frame_data, FrameData::Integer(-5));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Integer);
        assert_eq!(frame.frame_data, FrameData::Integer(0));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Boolean);
        assert_eq!(frame.frame_data, FrameData::Boolean(true));

        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Boolean);
        assert_eq!(frame.frame_data, FrameData::Boolean(false));

        let frame = decode(&mut stream);
        assert_eq!(frame, Err(FrameError::Invalid), "invalid bool payload");

        //
        //Array
        //
        let mut stream = BufReader::new("*3\r\n:1\r\n+Two\r\n$5\r\nThree\r\n".as_bytes());
        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Array);
        let frame_data = FrameData::Nested(vec![
            Frame {
                frame_type: FrameID::Integer,
                frame_data: FrameData::Integer(1),
            },
            Frame {
                frame_type: FrameID::SimpleString,
                frame_data: FrameData::Simple("Two".to_string()),
            },
            Frame {
                frame_type: FrameID::BulkString,
                frame_data: FrameData::Bulk(5, "Three".to_string()),
            },
        ]);
        assert_eq!(frame.frame_data, frame_data);

        // nested 1
        let mut stream = BufReader::new("*2\r\n:1\r\n*1\r\n+Three\r\n".as_bytes());
        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Array);
        let frame_data = FrameData::Nested(vec![
            Frame {
                frame_type: FrameID::Integer,
                frame_data: FrameData::Integer(1),
            },
            Frame {
                frame_type: FrameID::Array,
                frame_data: FrameData::Nested(vec![Frame {
                    frame_type: FrameID::SimpleString,
                    frame_data: FrameData::Simple("Three".to_string()),
                }]),
            },
        ]);
        assert_eq!(frame.frame_data, frame_data);

        // nested 2
        let mut stream = BufReader::new("*3\r\n:1\r\n*1\r\n+Three\r\n-Err\r\n".as_bytes());
        let frame = decode(&mut stream).unwrap();
        assert_eq!(frame.frame_type, FrameID::Array);
        let frame_data = FrameData::Nested(vec![
            Frame {
                frame_type: FrameID::Integer,
                frame_data: FrameData::Integer(1),
            },
            Frame {
                frame_type: FrameID::Array,
                frame_data: FrameData::Nested(vec![Frame {
                    frame_type: FrameID::SimpleString,
                    frame_data: FrameData::Simple("Three".to_string()),
                }]),
            },
            Frame {
                frame_type: FrameID::SimpleError,
                frame_data: FrameData::Simple("Err".to_string()),
            },
        ]);
        assert_eq!(frame.frame_data, frame_data);
    }
}
