//! What Railway prints for a headless connection: a one-line JSON manifest on
//! stderr (observed live; Railway documents only that it exists). Herdr reads
//! exactly one field from it, `build_expires_at`, the VM's authoritative
//! deadline, and keeps nothing else: its preview and claim links act on the
//! VM or its owner and are never stored, logged, or opened. Anything missing,
//! malformed, or implausible is ignored, never guessed at.

use super::WINDOW_SECONDS;

/// Slack for a deadline that is a little later than the documented window.
const PLAUSIBLE_EXTRA: u64 = 5 * 60;

/// The deadline in `output`, in Unix seconds, if some line is a JSON object
/// with an RFC 3339 `build_expires_at` that can belong to a VM created within
/// the documented window ending after `now`.
pub(super) fn deadline(output: &str, now: u64) -> Option<u64> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{') && line.len() <= 8192)
        .find_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            let text = value.get("build_expires_at")?.as_str()?;
            let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
            u64::try_from(at.timestamp()).ok()
        })
        .filter(|&at| at <= now.saturating_add(WINDOW_SECONDS + PLAUSIBLE_EXTRA))
}

/// `text` with every `http(s)://…` word replaced, for remote output shown to
/// the user: Railway's manifest can sit in a failure's output tail.
pub(super) fn hide_links(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            line.split(' ')
                .map(|word| {
                    // JSON puts the link after a quote or colon.
                    match word.find("http://").or_else(|| word.find("https://")) {
                        Some(start) => format!("{}[link hidden]", &word[..start]),
                        None => word.to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
