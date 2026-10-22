//! The `[chat]` table: what chats started from the app run with.
//!
//! ```toml
//! [chat.env]
//! GH_HOST = "mycompany.ghe.com"
//! ANTHROPIC_BASE_URL = "https://llm.example.com"
//! ```
//!
//! `env` goes into the environment of the tab a chat starts in, on this
//! machine or an SSH host, so an agent reaches a company endpoint that the
//! pane's shell does not set up. Values sit in the config file as written,
//! so a token belongs in the shell or the agent's own sign-in instead.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Variables kept, and the bytes their names and values take together, so
/// a `tab.create` request stays small.
pub(crate) const MAX_ENV: usize = 64;
pub(crate) const MAX_ENV_BYTES: usize = 16 * 1024;
/// Names the app sets itself, which the table may not replace.
const RESERVED: [&str; 1] = [crate::chat::PROGRAM_ENV];

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ChatConfig {
    pub env: BTreeMap<String, String>,
}

impl ChatConfig {
    /// Refuses a table a tab's environment cannot take.
    pub(super) fn check(&self) -> Result<(), crate::chat::Error> {
        check_env(&self.env)
    }
}

/// Every name a valid variable name and not one the app sets, every value
/// free of NUL, and the whole within [`MAX_ENV`] and [`MAX_ENV_BYTES`].
pub(crate) fn check_env(env: &BTreeMap<String, String>) -> Result<(), crate::chat::Error> {
    use crate::chat::Error;
    if env.len() > MAX_ENV {
        return Err(Error::EnvironmentSize);
    }
    let bytes: usize = env
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum();
    if bytes > MAX_ENV_BYTES {
        return Err(Error::EnvironmentSize);
    }
    for (name, value) in env {
        let valid = name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        if !valid || RESERVED.contains(&name.as_str()) || value.contains('\0') {
            return Err(Error::EnvironmentName { name: name.clone() });
        }
    }
    Ok(())
}

/// The table as the Settings field shows it: `NAME=value` pairs separated
/// by spaces, a value with spaces or quotes in double quotes.
pub(crate) fn env_text(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(name, value)| {
            if value.is_empty() || value.contains([' ', '"', '\\', '\t']) {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                format!("{name}=\"{escaped}\"")
            } else {
                format!("{name}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pairs the Settings field holds, or `None` when one is malformed.
pub(crate) fn parse_env(text: &str) -> Option<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            break;
        }
        let mut name = String::new();
        while let Some(ch) = chars.next_if(|ch| *ch != '=' && !ch.is_whitespace()) {
            name.push(ch);
        }
        chars.next_if_eq(&'=')?;
        let mut value = String::new();
        if chars.next_if_eq(&'"').is_some() {
            loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => value.push(chars.next()?),
                    ch => value.push(ch),
                }
            }
            if chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                return None;
            }
        } else {
            while let Some(ch) = chars.next_if(|ch| !ch.is_whitespace()) {
                value.push(ch);
            }
        }
        env.insert(name, value);
    }
    check_env(&env).ok().map(|()| env)
}

#[cfg(test)]
mod tests;
