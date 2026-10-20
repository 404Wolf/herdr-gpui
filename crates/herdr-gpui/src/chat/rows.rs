//! Prepared off-thread. Every Markdown/output line is a virtual row so a long
//! tool result cannot turn a single visible item into thousands of elements.
use crate::release_notes::{self, Line};
use herdr_client::chat::{Conversation, Part, Role};
use std::sync::Arc;

const MAX_ROWS: usize = 8192;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Body {
    Heading { role: Role, copy: Arc<str> },
    Text { role: Role, line: Line },
    Actions { group: String, count: usize },
    Tool { name: String, state: &'static str },
    Notice(String),
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub key: String,
    pub group: Option<String>,
    pub body: Body,
}

pub(super) fn prepare(conversation: &Conversation) -> Arc<[Row]> {
    let mut rows = Vec::new();
    'turns: for turn in &conversation.turns {
        let copy: Arc<str> = turn
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
            .into();
        rows.push(Row {
            key: format!("{}:heading", turn.id),
            group: None,
            body: Body::Heading {
                role: turn.role,
                copy,
            },
        });
        let tools: Vec<_> = turn
            .parts
            .iter()
            .filter(|p| matches!(p, Part::Tool { .. }))
            .collect();
        let group = format!("{}:tools", turn.id);
        let mut drew_tools = false;
        for (part_index, part) in turn.parts.iter().enumerate() {
            if let Part::Text(text) = part {
                for (i, line) in release_notes::parse(text).into_iter().enumerate() {
                    rows.push(Row {
                        key: format!("{}:{part_index}:{i}", turn.id),
                        group: None,
                        body: Body::Text {
                            role: turn.role,
                            line,
                        },
                    });
                    if rows.len() >= MAX_ROWS {
                        break 'turns;
                    }
                }
            } else if !drew_tools {
                drew_tools = true;
                rows.push(Row {
                    key: group.clone(),
                    group: None,
                    body: Body::Actions {
                        group: group.clone(),
                        count: tools.len(),
                    },
                });
                if rows.len() >= MAX_ROWS {
                    break 'turns;
                }
                for tool in &tools {
                    let Part::Tool {
                        id,
                        name,
                        input,
                        output,
                        failed,
                    } = tool
                    else {
                        continue;
                    };
                    rows.push(Row {
                        key: format!("{group}:{id}"),
                        group: Some(group.clone()),
                        body: Body::Tool {
                            name: crate::notifications::safe_text(name, 128),
                            state: if *failed {
                                "Failed"
                            } else if output.is_some() {
                                "Done"
                            } else {
                                "In progress"
                            },
                        },
                    });
                    if rows.len() >= MAX_ROWS {
                        break 'turns;
                    }
                    for (i, line) in release_notes::parse(&format!(
                        "```\n{input}\n{}\n```",
                        output.as_deref().unwrap_or("Waiting for result…")
                    ))
                    .into_iter()
                    .enumerate()
                    {
                        rows.push(Row {
                            key: format!("{group}:{id}:{i}"),
                            group: Some(group.clone()),
                            body: Body::Text {
                                role: Role::Assistant,
                                line,
                            },
                        });
                        if rows.len() >= MAX_ROWS {
                            break 'turns;
                        }
                    }
                }
            }
            if rows.len() >= MAX_ROWS {
                break;
            }
        }
        if rows.len() >= MAX_ROWS {
            break;
        }
    }
    if rows.len() >= MAX_ROWS {
        rows.truncate(MAX_ROWS);
        rows.push(Row {
            key: "row-limit".into(),
            group: None,
            body: Body::Notice("Display limit reached. Use Terminal for more history.".into()),
        });
    }
    rows.into()
}
