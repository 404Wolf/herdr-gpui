use super::*;
use serde_json::json;

mod parser;
mod store;

const ID: &str = "00000000-0000-4000-8000-000000000001";
fn jsonl(records: &[serde_json::Value]) -> Vec<u8> {
    records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>()
        .into_bytes()
}
fn message(
    kind: &str,
    uuid: &str,
    parent: Option<&str>,
    content: serde_json::Value,
) -> serde_json::Value {
    json!({"type":kind,"uuid":uuid,"parentUuid":parent,"sessionId":ID,"message":{"content":content}})
}
