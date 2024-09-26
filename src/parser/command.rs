use crate::parser::Frame;
use std::collections::HashMap;

pub(crate) enum CommandType {
    PING,
    GET,
    SET,
    DEL,
    EXPIRE,
    ERROR, // This isn't a command per se. But it is used to send erroneous responses back to the user.
}

pub(crate) struct Command {
    pub(crate) command_type: CommandType,
    pub(crate) args: Vec<String>,
}

impl Command {
    pub(crate) fn new(cmd_type: CommandType, args: &Vec<String>) -> Self {
        Command {
            command_type: cmd_type,
            args: args.to_owned(),
        }
    }

    pub(crate) fn make_redis_command_map() -> HashMap<&'static str, CommandType> {
        let mut map = HashMap::new();
        map.insert("PING", CommandType::PING);
        map.insert("GET", CommandType::GET);
        map.insert("SET", CommandType::SET);
        map.insert("DEL", CommandType::DEL);
        map.insert("EXPIRE", CommandType::EXPIRE);
        map
    }

    pub(crate) fn parse_ping_command(frames: &[Frame]) -> Command {
        if frames.len() > 2 {
            return Command {
                command_type: CommandType::ERROR,
                args: vec!["PING command must have at most 1 argument".to_string()],
            };
        }

        let mut ping_cmd = Command {
            command_type: CommandType::PING,
            args: vec!["PONG".to_string()],
        };

        if frames.len() == 2 {
            let ping_msg = frames[1].get_bulk().unwrap();
            ping_cmd.args[0] = ping_msg.to_string();
        }

        ping_cmd
    }
}
