/// Everything that can go wrong talking to Discord.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No `discord-ipc-N` socket accepted a connection: the Discord desktop
    /// app isn't running (or runs sandboxed somewhere we don't look).
    #[error("Discord isn't running (no IPC socket found)")]
    NotRunning,
    #[error("Discord IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A frame that doesn't follow the protocol: bad opcode, oversized
    /// length, or a payload that isn't the JSON we expect.
    #[error("malformed Discord IPC message: {0}")]
    Protocol(String),
    /// Discord answered a command with an `ERROR` event (e.g. an activity
    /// field it didn't accept).
    #[error("Discord rejected the request ({code}): {message}")]
    Rejected { code: i64, message: String },
    /// Discord closed the connection, e.g. after a handshake with an
    /// unknown client ID.
    #[error("Discord closed the connection ({code}): {message}")]
    Closed { code: i64, message: String },
    /// Discord accepted the connection but didn't answer in time.
    #[error("timed out waiting for Discord")]
    Timeout,
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Protocol(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
