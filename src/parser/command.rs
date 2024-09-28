use crate::parser::Frame;
use std::collections::HashMap;

#[derive(Eq, PartialEq, Debug)]
pub(crate) enum CommandType {
    PING,
    GET,
    SET,
    DEL,
    EXPIRE,
    ERROR, // This isn't a command per se. But it is used to send erroneous responses back to the user.
}

#[derive(Eq, PartialEq, Debug)]
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

    pub(crate) fn parse_get_command(frames: &[Frame]) -> Command {
        if frames.len() != 2 {
            return Command {
                command_type: CommandType::ERROR,
                args: vec!["GET command must have at exactly 1 argument".to_string()],
            };
        }

        let set_cmd = Command {
            command_type: CommandType::GET,
            args: vec![frames[1].get_bulk().unwrap().to_string()],
        };

        set_cmd
    }

    pub(crate) fn parse_set_command(frames: &[Frame]) -> Command {
        // note: we can unwrap get_bulk in this function because the frame
        // has been checked upfront. @TODO: maybe refactor to give a number instead of an option, then.
        let len = frames.len();
        if len != 3 && len != 5 {
            return Command {
                command_type: CommandType::ERROR,
                args: vec!["SET should take 2 or 4 arguments".to_string()],
            };
        }
        let key = frames[1].get_bulk().unwrap();
        let value = frames[2].get_bulk().unwrap();

        // check if we've got the right option to set the time in millis
        if len == 5 {
            let ping_opt = frames[3].get_bulk().unwrap();
            if ping_opt.to_uppercase() == "PX" {
                let expiration = frames[4].get_bulk().unwrap();
                // also check if expiration can be converted to a number, because we do not want the caller of this method to check anything
                // Ensure that expiration is convertible to a number
                if expiration.parse::<u64>().is_err() {
                    return Command {
                        command_type: CommandType::ERROR,
                        args: vec!["expiration should be a valid number".to_string()],
                    };
                }
                return Command {
                    command_type: CommandType::SET,
                    args: vec![key.to_string(), value.to_string(), expiration.to_string()],
                };
            }
            return Command {
                command_type: CommandType::ERROR,
                args: vec![format!("unknown option '{}' for SET command", ping_opt)],
            };
        }
        Command {
            command_type: CommandType::SET,
            args: vec![key.to_string(), value.to_string()],
        }
    }

    pub(crate) fn parse_del_command(frames: &[Frame]) -> Command {
        // note: we can unwrap get_bulk in this function because the frame
        // has been checked upfront. @TODO: maybe refactor to give a number instead of an option, then.
        let len = frames.len();
        if len <2  {
            return Command {
                command_type: CommandType::ERROR,
                args: vec!["DEL command must at least one arg".to_string()],
            };
        }
        
        let mut keys = Vec::with_capacity(len-1);
        for i in 1..len {
            keys.push(frames[i].get_bulk().unwrap().to_string());
        }
        
        Command {
            command_type: CommandType::DEL,
            args: keys,
        }
    }

    pub(crate) fn parse_expire_command(frames: &[Frame]) -> Command {
        unimplemented!("TODO: implement later")
    }
}
