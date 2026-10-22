use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error(transparent)]
    Control(#[from] crate::control::Error),
    #[cfg(feature = "server")]
    #[error(transparent)]
    Hub(#[from] crate::hub::Error),
    #[error("remote control I/O failed")]
    Io(#[from] io::Error),
    // keep-sorted end
}
