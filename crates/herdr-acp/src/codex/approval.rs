//! Codex's approval requests as ACP permission requests, and the answers
//! back.

use acp::v1 as wire;
use serde_json::{Value, json};

use crate::harness::out;

/// A question waiting for its answer: what each option tells Codex.
#[derive(Debug)]
pub(super) struct Ask {
    /// Codex's request id, answered with the reply.
    pub request: Value,
    choices: Vec<(String, Value)>,
    /// What a dropped question tells Codex.
    dropped: Value,
}

/// The tool call and options to offer for a server request, and the
/// question that keeps them; `None` for a request this program does not
/// answer.
pub(super) fn ask(
    method: &str,
    request: Value,
    params: &Value,
) -> Option<(wire::ToolCallUpdate, Vec<wire::PermissionOption>, Ask)> {
    let field = |key: &str| params.get(key).and_then(Value::as_str);
    let call = field("itemId").unwrap_or("approval").to_owned();
    let reason = field("reason").map(str::to_owned);
    let decision = |decision: &str| json!({"decision": decision});
    let (title, options, dropped) = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let title = match (method, field("command")) {
                (_, Some(command)) => out::command_title(command),
                ("item/fileChange/requestApproval", None) => reason
                    .clone()
                    .unwrap_or_else(|| "Apply these changes?".to_owned()),
                _ => reason
                    .clone()
                    .unwrap_or_else(|| "Run this command?".to_owned()),
            };
            let options = vec![
                choice(
                    "accept",
                    "Yes",
                    wire::PermissionOptionKind::AllowOnce,
                    decision("accept"),
                ),
                choice(
                    "session",
                    "Yes, for this session",
                    wire::PermissionOptionKind::AllowAlways,
                    decision("acceptForSession"),
                ),
                choice(
                    "decline",
                    "No",
                    wire::PermissionOptionKind::RejectOnce,
                    decision("decline"),
                ),
            ];
            (title, options, decision("cancel"))
        }
        "item/permissions/requestApproval" => {
            let requested = params.get("permissions").cloned().unwrap_or(Value::Null);
            let grant = |scope: &str| json!({"permissions": requested, "scope": scope});
            let options = vec![
                choice(
                    "turn",
                    "Yes, for this turn",
                    wire::PermissionOptionKind::AllowOnce,
                    grant("turn"),
                ),
                choice(
                    "session",
                    "Yes, for this session",
                    wire::PermissionOptionKind::AllowAlways,
                    grant("session"),
                ),
                choice(
                    "decline",
                    "No",
                    wire::PermissionOptionKind::RejectOnce,
                    json!({"permissions": {}, "scope": "turn"}),
                ),
            ];
            let refused = json!({"permissions": {}, "scope": "turn"});
            (
                reason.unwrap_or_else(|| "Allow more access?".to_owned()),
                options,
                refused,
            )
        }
        "item/tool/requestUserInput" => question(params)?,
        _ => return None,
    };
    let (options, choices) = options.into_iter().unzip();
    let fields = wire::ToolCallUpdateFields::new().title(Some(title));
    Some((
        wire::ToolCallUpdate::new(call, fields),
        options,
        Ask {
            request,
            choices,
            dropped,
        },
    ))
}

impl Ask {
    /// The JSON-RPC response for the session's answer.
    pub(super) fn answer(self, result: &Result<Value, wire::Error>) -> Value {
        let reply = out::selected(result)
            .and_then(|id| {
                self.choices
                    .into_iter()
                    .find(|(known, _)| *known == id)
                    .map(|(_, reply)| reply)
            })
            .unwrap_or(self.dropped);
        json!({"id": self.request, "result": reply})
    }
}

type Offered = (wire::PermissionOption, (String, Value));

fn choice(id: &str, name: &str, kind: wire::PermissionOptionKind, reply: Value) -> Offered {
    (
        wire::PermissionOption::new(id.to_owned(), name, kind),
        (id.to_owned(), reply),
    )
}

/// One question with choices becomes the options; anything else is left to
/// the chat, answered with nothing.
fn question(params: &Value) -> Option<(String, Vec<Offered>, Value)> {
    let unanswered = json!({"answers": {}});
    let questions = params.get("questions")?.as_array()?;
    let [only] = questions.as_slice() else {
        let options = vec![choice(
            "decline",
            "Answer in the chat",
            wire::PermissionOptionKind::RejectOnce,
            unanswered.clone(),
        )];
        return Some(("Codex has questions".to_owned(), options, unanswered));
    };
    let id = only.get("id")?.as_str()?;
    let asked = only
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or("Codex asks");
    let mut options: Vec<Offered> = only
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("label").and_then(Value::as_str))
        .enumerate()
        .map(|(index, label)| {
            choice(
                &format!("answer-{index}"),
                label,
                wire::PermissionOptionKind::AllowOnce,
                json!({"answers": {id: {"answers": [label]}}}),
            )
        })
        .collect();
    options.push(choice(
        "decline",
        "Answer in the chat",
        wire::PermissionOptionKind::RejectOnce,
        unanswered.clone(),
    ));
    Some((asked.to_owned(), options, unanswered))
}
