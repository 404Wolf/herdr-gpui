//! Commands this process answers from the app's files and its own code,
//! whether or not Herdr UI runs.
use super::Output;

fn todo() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "Not implemented yet".into(),
    })
}

/// Every command `run` accepts, with its name and label.
pub(super) fn commands() -> crate::Result<Output> {
    let commands: Vec<_> = crate::controls::COMMANDS
        .iter()
        .map(|info| serde_json::json!({ "name": info.name, "label": info.label }))
        .collect();
    let text = crate::controls::COMMANDS
        .iter()
        .map(|info| format!("{:<28}{}", info.name, info.label))
        .collect::<Vec<_>>()
        .join("\n");
    Output::data(text, &commands)
}

pub(super) fn processes(_: Option<&str>) -> crate::Result<Output> {
    todo()
}

pub(super) fn search(_: bool, _: &str, _: usize) -> crate::Result<Output> {
    todo()
}

pub(super) fn logs(_: bool, _: usize) -> crate::Result<Output> {
    todo()
}

pub(super) fn diagnostics(_: &std::path::Path) -> crate::Result<Output> {
    todo()
}
