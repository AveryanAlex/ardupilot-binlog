#[derive(Debug, thiserror::Error)]
pub enum BinlogError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid format character: {0}")]
    InvalidFormat(char),

    #[error("no FMT definition for message type {0}")]
    UnknownType(u8),

    #[error("unexpected end of data")]
    UnexpectedEof,

    #[error("payload too short for field type")]
    PayloadTooShort,
}
