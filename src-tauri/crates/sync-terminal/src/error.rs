//! What can go wrong, in words the layer above can turn into a sentence.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no terminal is open under that name")]
    Unknown,

    /// The folder a terminal was asked to start in is not one.
    ///
    /// Checked here rather than left to the shell, because a pty whose child
    /// dies on its first instruction looks exactly like a shell that exited,
    /// and the person is shown an empty black box instead of the reason.
    #[error("{0} is not a folder this terminal can start in")]
    NoSuchFolder(PathBuf),

    #[error("could not open a terminal: {0}")]
    Open(String),

    #[error("the terminal has ended")]
    Ended,

    #[error("could not write to the terminal: {0}")]
    Write(#[from] std::io::Error),
}
