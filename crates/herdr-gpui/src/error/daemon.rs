//! How a daemon's JSON error reads in a message.

/// The daemon's own message, else its code, for an error it reported.
pub(super) fn daemon_error_message(error: &serde_json::Value) -> &str {
    error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| error.as_str())
        .or_else(|| error.get("code").and_then(serde_json::Value::as_str))
        .filter(|message| !message.is_empty())
        .unwrap_or("Invalid daemon error")
}
