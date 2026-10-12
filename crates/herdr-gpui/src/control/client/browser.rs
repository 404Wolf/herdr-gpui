//! `browser ...` and `feedback`.
use super::{Context, Output, caller, notes_daemon, rejected, send, tabs, unexpected};
use crate::{
    browser::{LocalFile, WebUrl},
    cli::control::BrowserCommand,
    control::protocol::{
        BrowserOpen, ErrorCode, FeedbackRequest, MAX_WAIT_SECONDS, OpenedIn, Page, Request,
        Response,
        tabs::{Reply, TabOp},
    },
};
use std::path::Path;

/// A file that exists is shown as a local page; anything else is an address.
fn page(target: &str) -> crate::Result<(Page, String)> {
    let path = Path::new(target);
    if !target.contains("://") && path.is_file() {
        let path = path.canonicalize()?;
        let home = crate::config::home().ok();
        LocalFile::new(&path, home.as_deref())?;
        let text = path
            .to_str()
            .ok_or(crate::Error::InvalidLocalPage)?
            .to_owned();
        return Ok((Page::File(text.clone()), text));
    }
    let url = WebUrl::from_typed(target)?;
    Ok((Page::Url(url.as_str().to_owned()), url.as_str().to_owned()))
}

fn open(
    target: &str,
    workspace: Option<String>,
    focus: bool,
    context: &Context,
) -> crate::Result<Output> {
    let (page, shown) = page(target)?;
    let request = Request::Open(BrowserOpen {
        page,
        caller: crate::control::protocol::Caller {
            window: context.window,
            ..caller(workspace)?
        },
        focus,
    });
    let response = send(&request)?;
    let text = match &response {
        Response::Opened {
            opened_in: OpenedIn::Tab,
            workspace_id: Some(id),
        } => format!("Opened {shown} in a browser tab of workspace {id}"),
        Response::Opened {
            opened_in: OpenedIn::Tab,
            workspace_id: None,
        } => format!("Opened {shown} in a browser tab"),
        Response::Opened {
            opened_in: OpenedIn::SystemBrowser,
            ..
        } => format!(
            "Opened {shown} in the system browser; this platform's build has no browser tabs"
        ),
        _ => return Err(rejected(response)),
    };
    Output::data(text, &response)
}

fn reload() -> crate::Result<Output> {
    match send(&Request::Reload(caller(None)?))? {
        Response::Reloaded { tabs: 0 } => Err(crate::Error::ControlRejected {
            code: ErrorCode::InvalidRequest,
            message: "This pane has no browser tabs open".into(),
        }),
        Response::Reloaded { tabs } => Output::data(
            format!("Reloaded {tabs} browser tab(s)"),
            &serde_json::json!({ "tabs": tabs }),
        ),
        response => Err(rejected(response)),
    }
}

/// The user's notes for this pane: page, review, and terminal notes alike
/// are kept for it whenever they could not be typed into the pane.
pub(super) fn feedback(wait: u64, _: &Context) -> crate::Result<Output> {
    let pane_id = super::env_id("HERDR_PANE_ID").ok_or(crate::Error::ControlRejected {
        code: ErrorCode::InvalidRequest,
        message: "feedback runs in the Herdr pane that should receive the notes".into(),
    })?;
    let request = Request::Feedback(FeedbackRequest {
        pane_id,
        daemon_socket: Some(notes_daemon()?),
        wait_seconds: wait.min(MAX_WAIT_SECONDS),
    });
    match send(&request)? {
        // Notes keep their line breaks: they are Markdown for the agent.
        Response::Feedback { text: Some(text) } => {
            Output::data(text.trim_end(), &serde_json::json!({ "notes": text }))
        }
        Response::Feedback { text: None } => Ok(Output::data(
            "No notes from the user yet",
            &serde_json::json!({ "notes": null }),
        )?
        .nothing()),
        response => Err(rejected(response)),
    }
}

/// The text of a page tab.
fn text(id: u64, context: &Context) -> crate::Result<Output> {
    match context.call(Request::Tab, TabOp::Text { id })? {
        Response::Tab {
            reply: Reply::Text { text },
        } => Output::data(text.clone(), &serde_json::json!({ "id": id, "text": text })),
        response => Err(unexpected(response)),
    }
}

/// The screenshot is written by the app, so the path is made absolute here,
/// where the caller's working directory is known.
fn screenshot(id: u64, path: &Path, context: &Context) -> crate::Result<Output> {
    let path = std::path::absolute(path)?;
    let text = path
        .to_str()
        .ok_or(crate::Error::InvalidLocalPage)?
        .to_owned();
    context.act(Request::Tab, TabOp::Screenshot { id, path: text })
}

pub(super) fn run(command: BrowserCommand, context: &Context) -> crate::Result<Output> {
    match command {
        // Answered before any request; see `client::run`.
        BrowserCommand::Help | BrowserCommand::Skill => Ok(Output::message("")),
        BrowserCommand::Open {
            target,
            workspace,
            focus,
        } => open(&target, workspace, focus, context),
        BrowserCommand::Reload => reload(),
        BrowserCommand::Feedback { wait } => feedback(wait, context),
        BrowserCommand::List => tabs::list(true, context),
        BrowserCommand::Close { id } => context.act(Request::Tab, TabOp::Close { id }),
        BrowserCommand::Navigate { id, url } => {
            let url = WebUrl::from_typed(&url)?;
            context.act(
                Request::Tab,
                TabOp::Navigate {
                    id,
                    url: url.as_str().to_owned(),
                },
            )
        }
        BrowserCommand::Back { id } => context.act(Request::Tab, TabOp::Back { id }),
        BrowserCommand::Forward { id } => context.act(Request::Tab, TabOp::Forward { id }),
        BrowserCommand::Text { id } => text(id, context),
        BrowserCommand::Screenshot { id, path } => screenshot(id, &path, context),
    }
}
