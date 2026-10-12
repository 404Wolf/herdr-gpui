//! `diagnostics bundle`, answered by this process: one text file for a bug
//! report. It holds the build, the platform, where the settings are, what
//! loading them reports, the user's overrides, and recent log records, all
//! passed through [`redact`]. Herdr's own config, stored credentials, and the
//! keychain are never read.
mod redact;
#[cfg(test)]
mod tests;

use super::{Output, logs};
use crate::config::{Config, daemon_config_path};
use redact::Redactor;
use std::{
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Log records the bundle carries.
const LOG_RECORDS: usize = 500;
/// The overrides file is small; anything past this is not ours to send.
const MAX_OVERRIDES: usize = 64 * 1024;

pub(crate) fn diagnostics(output: &Path) -> crate::Result<Output> {
    if output.symlink_metadata().is_ok() {
        return Err(crate::Error::BundleExists {
            path: output.to_owned(),
        });
    }
    let redactor = Redactor::new(crate::config::home().ok().as_deref());
    let text = redactor.redact(&gather());
    write_new(output, &text)?;
    Ok(Output::message(format!(
        "Wrote {}; read it before you share it",
        output.display()
    )))
}

fn gather() -> String {
    let mut text = String::from("Herdr UI diagnostics\n\n");
    let _ = writeln!(text, "version: {}", crate::constants::APP_VERSION);
    let _ = writeln!(
        text,
        "platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for line in crate::cli::build_info().lines() {
        let _ = writeln!(text, "build {line}");
    }
    text.push_str("\n== Settings files ==\n");
    let files: [(&str, crate::Result<PathBuf>); 3] = [
        ("local", Config::local_path()),
        ("managed", Config::path()),
        ("herdr", Ok(daemon_config_path(|key| std::env::var_os(key)))),
    ];
    for (name, path) in &files {
        match path {
            Ok(path) => {
                let state = if path.exists() { "" } else { " (absent)" };
                let _ = writeln!(text, "{name}: {}{state}", path.display());
            }
            Err(error) => {
                let _ = writeln!(text, "{name}: {error}");
            }
        }
    }
    text.push_str("\n== Settings check (fonts not checked) ==\n");
    match Config::check_files() {
        Ok(config) => {
            let _ = writeln!(
                text,
                "{}",
                config
                    .diagnostic()
                    .unwrap_or_else(|| "No problems found".into())
            );
        }
        Err(error) => {
            let _ = writeln!(text, "{error}");
        }
    }
    text.push_str("\n== config-gpui.local.toml ==\n");
    match &files[0].1 {
        Ok(path) => text.push_str(&overrides(path)),
        Err(error) => {
            let _ = writeln!(text, "{error}");
        }
    }
    let _ = writeln!(text, "\n== Last {LOG_RECORDS} log records ==");
    text.push_str(&log_records());
    text
}

fn overrides(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(contents) if contents.len() > MAX_OVERRIDES => {
            format!("(omitted: larger than {MAX_OVERRIDES} bytes)\n")
        }
        Ok(contents) => summary(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "(absent)\n".into(),
        Err(_) => "(unreadable)\n".into(),
    }
}

/// Settings whose text values name a choice, never a secret, and are kept.
const PLAIN_TEXT_KEYS: [&str; 10] = [
    "theme",
    "family",
    "mode",
    "layout",
    "position",
    "placement",
    "channel",
    "open_links_in",
    "style",
    "sort",
];

/// The overrides as `key = value` lines, never the file itself: numbers,
/// booleans, and the text of plain choices are kept, and any other text is
/// replaced by its length, so a secret under any key name, in any TOML form,
/// stays out of the bundle. A file that does not parse is not quoted, as a
/// parser error would echo its text.
fn summary(contents: &str) -> String {
    let Ok(table) = contents.parse::<toml::Table>() else {
        return "(does not parse as TOML)\n".into();
    };
    let mut lines = Vec::new();
    flatten("", &toml::Value::Table(table), &mut lines);
    if lines.is_empty() {
        return "(no settings)\n".into();
    }
    lines.join("\n") + "\n"
}

fn flatten(key: &str, value: &toml::Value, lines: &mut Vec<String>) {
    let name = key.rsplit('.').next().unwrap_or_default();
    let shown = match value {
        toml::Value::Table(table) => {
            for (child, value) in table {
                let child = if key.is_empty() {
                    child.clone()
                } else {
                    format!("{key}.{child}")
                };
                flatten(&child, value, lines);
            }
            return;
        }
        toml::Value::Boolean(_) | toml::Value::Integer(_) | toml::Value::Float(_) => {
            value.to_string()
        }
        toml::Value::String(text)
            if PLAIN_TEXT_KEYS.iter().any(|plain| name.ends_with(plain))
                && !redact::secret_name(name) =>
        {
            toml::Value::String(text.chars().take(200).collect()).to_string()
        }
        toml::Value::String(text) => format!("<text, {} characters>", text.chars().count()),
        toml::Value::Array(items) => format!("<list of {}>", items.len()),
        toml::Value::Datetime(_) => "<date>".into(),
    };
    lines.push(format!("{key} = {shown}"));
}

fn log_records() -> String {
    let Some(current) = crate::diagnostics::log_file() else {
        return "(no state directory)\n".into();
    };
    let previous = crate::diagnostics::previous_log_file(&current);
    match logs::tail(&previous, &current, LOG_RECORDS) {
        Ok(records) if records.is_empty() => "(no log yet)\n".into(),
        Ok(records) => {
            let records: Vec<_> = records.iter().map(Arc::as_ref).collect();
            format!("{}\n", logs::lines_of(&records))
        }
        Err(error) => format!("({error})\n"),
    }
}

/// Writes `text` to a new file at `path`, all at once, never replacing a file
/// that appeared meanwhile.
fn write_new(path: &Path, text: &str) -> crate::Result<()> {
    let failed = |source| crate::Error::BundleWrite {
        path: path.to_owned(),
        source,
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(failed)?;
    file.write_all(text.as_bytes()).map_err(failed)?;
    file.as_file().sync_all().map_err(failed)?;
    file.persist_noclobber(path).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            crate::Error::BundleExists {
                path: path.to_owned(),
            }
        } else {
            failed(error.error)
        }
    })?;
    Ok(())
}
