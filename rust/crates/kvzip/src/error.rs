use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The record does not fit in an empty segment even after compression.
    RecordTooLarge {
        record_bytes: u64,
        max_segment_bytes: u64,
    },
    InvalidOptions(String),
    /// The directory holds something kvzip did not write: a record that passed its checksum
    /// but does not decode, or a segment name no new segment can sort after.
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "I/O error: {error}"),
            Error::RecordTooLarge {
                record_bytes,
                max_segment_bytes,
            } => write!(
                f,
                "record needs {record_bytes} bytes but a segment holds at most {max_segment_bytes}"
            ),
            Error::InvalidOptions(message) => write!(f, "invalid options: {message}"),
            Error::Corrupt(message) => write!(f, "corrupt store: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}
