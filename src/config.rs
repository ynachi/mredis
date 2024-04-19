use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "mredis")]
#[command(version = "0.1.0")]
#[command(about = "Simple distributed cache server", long_about = None)]
pub struct Config {
    /// Hostname or IP address to listen on.
    #[clap(name = "hostname", long, short, default_value = "127.0.0.1")]
    pub ip_addr: String,

    /// Port to listen on.
    #[clap(long, short, default_value = "6379")]
    pub port: u16,

    /// Pre-allocated storage capacity.
    #[clap(long, short, default_value = "1000000")]
    pub capacity: usize,

    /// Number of storage shards.
    #[clap(name = "shard", long, short, default_value = "8")]
    pub shard_count: usize,

    /// Network read and write buffer size.
    #[clap(name = "buffer", long, short, default_value = "8192")]
    pub network_buffer_size: usize,

    /// Max log level.
    #[clap(short, long, default_value_t, value_enum)]
    pub verbosity: Verbosity,
}

/// Verbosity logging verbosity
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug, Default)]
pub enum Verbosity {
    /// Max level Error
    Error,
    /// Max level Warning
    Warn,
    /// Max level Info
    #[default]
    Info,
    /// Max level Debug
    Debug,
    /// Higher level Trace
    Trace,
}

pub fn parse_log_level(level: Verbosity) -> tracing::Level {
    match level {
        Verbosity::Error => tracing::Level::ERROR,
        Verbosity::Warn => tracing::Level::WARN,
        Verbosity::Info => tracing::Level::INFO,
        Verbosity::Debug => tracing::Level::DEBUG,
        Verbosity::Trace => tracing::Level::TRACE,
    }
}
