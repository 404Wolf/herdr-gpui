use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error(transparent)]
    Control(#[from] crate::control::Error),
    #[error("remote control I/O failed")]
    Io(#[from] io::Error),
    // keep-sorted end
}
