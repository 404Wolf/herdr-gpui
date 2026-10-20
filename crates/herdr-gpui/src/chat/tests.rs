use super::*;
use core::prelude::v1::test;
use herdr_client::chat::{Conversation, Part, Role, Turn};
mod targeting;

#[test]
fn tool_output_is_prepared_as_hidden_virtual_rows() {
    let conversation = Conversation {
        turns: vec![Turn {
            id: "turn".into(),
            role: Role::Assistant,
            parts: vec![
                Part::Text("### Answer\nUse `cargo test`.".into()),
                Part::Tool {
                    id: "call".into(),
                    name: "Bash".into(),
                    input: "cargo test".into(),
                    output: Some("passed".into()),
                    failed: false,
                },
            ],
        }],
        ..Default::default()
    };
    let prepared = rows::prepare(&conversation);
    assert!(
        prepared
            .iter()
            .any(|row| matches!(row.body, rows::Body::Actions { count: 1, .. }))
    );
    assert!(
        prepared
            .iter()
            .any(|row| row.group.as_deref() == Some("turn:tools"))
    );
    let keys: std::collections::HashSet<_> = prepared.iter().map(|row| &row.key).collect();
    assert_eq!(keys.len(), prepared.len());
}
