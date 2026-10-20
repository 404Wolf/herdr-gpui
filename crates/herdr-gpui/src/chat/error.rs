#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cannot identify exactly one foreground Claude process for this pane")]
    SessionProcesses,
    #[error("Chat currently supports local Claude Code panes in release sessions")]
    Unsupported,
    #[error("The pane, terminal, or Claude session changed; nothing was sent")]
    TargetChanged,
    #[error("Claude is not ready for a message; use Terminal")]
    NotReady,
    #[error("Could not verify an empty Claude input box. Send or clear its draft in Terminal.")]
    InputOccupied,
    #[error("Enter a message of at most 20000 bytes, without terminal control characters")]
    InvalidPrompt,
    #[error("The chat worker stopped")]
    Stopped,
    #[error("Could not start the chat worker")]
    Thread(#[source] std::io::Error),
    #[error("Could not query Herdr")]
    Query(#[source] herdr_client::Error),
    #[error("Delivery was not confirmed. Check Terminal before sending again.")]
    Unconfirmed(#[source] herdr_client::Error),
    #[error("Herdr returned an invalid chat response")]
    Decode(#[source] serde_json::Error),
    #[error(transparent)]
    Transcript(#[from] herdr_client::chat::Error),
    #[error("The Claude config directory is unavailable")]
    Config,
}
pub type Result<T> = std::result::Result<T, Error>;
