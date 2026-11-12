use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not read audio file: {0}")]
    Decode(String),
    #[error("{0} is not a folder")]
    NotAFolder(PathBuf),
    #[error("this folder is already in the library (or inside a folder that is)")]
    FolderAlreadyAdded,
    #[error("not found: {0}")]
    NotFound(String),
    /// The detail is kept for logs; the message is what the GM reads.
    #[error("No speakers or headphones found, so nothing can play yet.")]
    NoDevice(String),
    /// A problem with a campaign bundle, in plain words.
    #[error("{0}")]
    Bundle(String),
    /// Something went wrong talking to an online service, in plain words.
    #[error("{0}")]
    Online(String),
    /// A sound that cannot be played, said in words for the GM.
    #[error("{0}")]
    Unplayable(String),
    #[error("playback failed: {0}")]
    Playback(String),
    #[error("the audio engine has stopped")]
    EngineGone,
}

pub type Result<T> = std::result::Result<T, Error>;
