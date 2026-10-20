use super::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    parent_uuid: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    is_sidechain: bool,
    #[serde(default)]
    is_meta: bool,
    #[serde(default)]
    is_compact_summary: bool,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    subtype: String,
    #[serde(default)]
    message: Message,
}

#[derive(Default, Deserialize)]
struct Message {
    #[serde(default)]
    content: Value,
    #[serde(default)]
    model: Option<String>,
}

/// Only complete JSONL records are consumed. An unfinished UTF-8/JSON line is
/// retried with the next read, never projected as a partial user message.
pub fn parse(bytes: &[u8], session: &str) -> Result<Conversation> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Limit);
    }
    let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else {
        return if bytes.len() > MAX_LINE_BYTES {
            Err(Error::Limit)
        } else {
            Ok(Conversation::default())
        };
    };
    if bytes.len() - end > MAX_LINE_BYTES {
        return Err(Error::Limit);
    }
    let mut records: Vec<Record> = Vec::new();
    for line in bytes[..=end]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > MAX_LINE_BYTES || records.len() >= MAX_RECORDS {
            return Err(Error::Limit);
        }
        let record: Record = serde_json::from_slice(line).map_err(Error::Record)?;
        if record.is_sidechain {
            continue;
        }
        if record.session_id.as_deref().is_some_and(|id| id != session) {
            return Err(Error::WrongSession);
        }
        records.push(record);
    }
    // Claude can retain abandoned branches. Follow the last recorded leaf,
    // rather than showing every user turn ever appended to the file.
    let index: HashMap<&str, usize> = records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.uuid.as_deref().map(|id| (id, i)))
        .collect();
    let mut selected = Vec::new();
    if records.iter().any(|r| r.parent_uuid.is_some()) {
        let mut next = records.iter().rposition(|r| r.uuid.is_some());
        let mut visited = HashSet::new();
        while let Some(i) = next {
            if !visited.insert(i) {
                return Err(Error::Identity);
            }
            selected.push(i);
            next = records[i]
                .parent_uuid
                .as_deref()
                .and_then(|id| index.get(id))
                .copied();
        }
        selected.reverse();
    } else {
        selected.extend((0..records.len()).filter(|i| {
            records[*i]
                .uuid
                .as_deref()
                .is_none_or(|id| index.get(id) == Some(i))
        }));
    }
    let mut result = Conversation::default();
    for i in selected {
        let record = &records[i];
        if record.subtype == "compact_boundary"
            || record.is_compact_summary
            || record.message.content.as_str().is_some_and(|s| {
                s.starts_with("<command-name>/clear</command-name>")
                    || (s.starts_with("<command-message>clear</command-message>")
                        && s.contains("<command-name>/clear</command-name>"))
            })
        {
            result.turns.clear();
            result.model = None;
            result.boundary = record.uuid.clone();
            continue;
        }
        if record.is_meta || !matches!(record.kind.as_str(), "user" | "assistant") {
            continue;
        }
        let role = if record.kind == "user" {
            Role::User
        } else {
            Role::Assistant
        };
        if role == Role::Assistant
            && let Some(model) = &record.message.model
        {
            result.model = Some(bounded(model, &mut result.limited));
        }
        let mut parts = Vec::new();
        match &record.message.content {
            Value::String(text) => push_text(text, &mut parts, &mut result.limited),
            Value::Array(blocks) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                push_text(text, &mut parts, &mut result.limited);
                            }
                        }
                        Some("tool_use") if role == Role::Assistant => {
                            let Some(id) = block.get("id").and_then(Value::as_str) else {
                                continue;
                            };
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("Tool");
                            let input =
                                block.get("input").map(Value::to_string).unwrap_or_default();
                            parts.push(Part::Tool {
                                id: bounded(id, &mut result.limited),
                                name: bounded(name, &mut result.limited),
                                input: bounded(&input, &mut result.limited),
                                output: None,
                                failed: false,
                            });
                        }
                        Some("tool_result") => {
                            let id = block.get("tool_use_id").and_then(Value::as_str);
                            let content = &block["content"];
                            let text = match content {
                                Value::String(s) => s.clone(),
                                Value::Array(items) => items
                                    .iter()
                                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                _ => String::new(),
                            };
                            let text = bounded(&text, &mut result.limited);
                            if let Some(Part::Tool { output, failed, .. }) = result.turns.iter_mut().rev()
                            .flat_map(|turn| turn.parts.iter_mut().rev())
                            .find(|part| matches!(part, Part::Tool { id: call, .. } if Some(call.as_str()) == id))
                        {
                            *output = Some(text);
                            *failed = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                        }
                        }
                        Some("image") => {
                            parts.push(Part::Text("[Image — view in Terminal]".into()))
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if parts.is_empty() {
            continue;
        }
        if role == Role::Assistant
            && let Some(last) = result.turns.last_mut().filter(|t| t.role == role)
        {
            last.parts.extend(parts);
        } else {
            result.turns.push(Turn {
                id: record.uuid.clone().unwrap_or_else(|| format!("record-{i}")),
                role,
                parts,
            });
        }
    }
    Ok(result)
}

fn push_text(text: &str, parts: &mut Vec<Part>, limited: &mut bool) {
    if text.is_empty()
        || text.starts_with("<command-")
        || text.starts_with("<local-command")
        || text.starts_with("<task-notification>")
    {
        return;
    }
    parts.push(Part::Text(bounded(text, limited)));
}
