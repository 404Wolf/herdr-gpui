//! Starts the agent and wires the session, the attach socket, Herdr reports,
//! and the in-pane view together.

use std::{path::PathBuf, thread};

use anyhow::Context as _;
use crossbeam_channel::{Sender, bounded};
use herdr_chat_protocol::{EndpointId, NoticeLevel};

use crate::{
    cli::{AgentChoice, Options},
    herdr::{Reporter, Target},
    process, rpc,
    session::{Config, Flow, Input, Session},
};

/// Inputs waiting for the session thread.
const INPUTS: usize = 1024;

struct Agent {
    args: Vec<std::ffi::OsString>,
    display: String,
    login_hint: Option<String>,
    program: std::ffi::OsString,
}

fn agent(choice: AgentChoice, name: Option<String>) -> Agent {
    match choice {
        AgentChoice::Builtin { agent, args: extra } => {
            let (program, mut args) = crate::agents::command(
                agent,
                Some(&crate::agents::search_path(
                    std::env::var_os("PATH").as_deref(),
                    std::env::home_dir().as_deref(),
                )),
            );
            args.extend(extra);
            Agent {
                args,
                display: name.unwrap_or_else(|| agent.display_name().to_owned()),
                login_hint: Some(agent.login_hint().to_owned()),
                program,
            }
        }
        AgentChoice::Command { program, args } => Agent {
            display: name.unwrap_or_else(|| {
                PathBuf::from(&program)
                    .file_name()
                    .map_or_else(|| "Agent".to_owned(), |n| n.to_string_lossy().into_owned())
            }),
            args,
            login_hint: None,
            program,
        },
    }
}

pub(crate) fn run(options: Options) -> anyhow::Result<()> {
    let cwd = match options.cwd {
        Some(cwd) => cwd,
        None => std::env::current_dir().context("could not read the current directory")?,
    };
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("no directory {}", cwd.display()))?;
    let agent = agent(options.agent, options.name);
    let spawned = process::spawn(&agent.program, &agent.args, &cwd)?;
    let agent_process = spawned.process.clone();
    let (inputs, receiver) = bounded::<Input>(INPUTS);

    let to_session = inputs.clone();
    rpc::spawn_reader(spawned.output, move |inbound| {
        to_session.send(Input::Agent(inbound)).is_ok()
    })?;
    let (outbox, _writer) = rpc::spawn_writer(spawned.input)?;
    let waiter = inputs.clone();
    let watched = agent_process.clone();
    thread::Builder::new()
        .name("acp-wait".to_owned())
        .spawn(move || {
            let code = watched.wait().ok().and_then(|status| status.code());
            let stderr = watched.stderr_tail();
            let _ = waiter.send(Input::AgentExited { code, stderr });
        })?;

    let mut notices = Vec::new();
    // Held to the end: dropping the server removes its socket.
    let (_server, endpoint) = if options.attach {
        listen(inputs.clone(), &mut notices)
    } else {
        (None, None)
    };
    let target = Target::from_env();
    let reporter = match &target {
        Some(target) => Some(Reporter::start(
            target.clone(),
            agent.display.clone(),
            endpoint.map(|id| id.to_string()),
        )?),
        None => None,
    };
    let report = reporter.as_ref().map(Reporter::handle);

    let config = Config {
        agent: agent.display,
        cwd,
        exit_with_agent: options.headless,
        load: options.load,
        login_hint: agent.login_hint,
        pane: target.map(|target| target.pane),
    };
    let session_thread = thread::Builder::new()
        .name("acp-session".to_owned())
        .spawn(move || {
            let mut session = Session::new(config, outbox, move |activity| {
                if let Some(report) = &report {
                    report.report(activity);
                }
            });
            for (level, text) in notices {
                session.notice(level, text);
            }
            session.start();
            for input in receiver {
                if session.handle(input) == Flow::Exit {
                    break;
                }
            }
        })?;

    // Headless, the session ends when the agent does; otherwise when the view quits.
    let result = if options.headless {
        Ok(())
    } else {
        let result = crate::tui::run(&inputs);
        let _ = inputs.send(Input::Shutdown);
        result
    };
    let _ = session_thread.join();
    agent_process.stop();
    if let Some(reporter) = reporter {
        reporter.close();
    }
    result
}

fn listen(
    inputs: Sender<Input>,
    notices: &mut Vec<(NoticeLevel, String)>,
) -> (Option<crate::attach::Server>, Option<EndpointId>) {
    let listening = crate::attach::address().and_then(|(address, id)| {
        crate::attach::Server::listen(&address, inputs).map(|server| (server, id))
    });
    match listening {
        Ok((server, id)) => (Some(server), Some(id)),
        Err(error) => {
            notices.push((
                NoticeLevel::Warning,
                format!("Herdr UI cannot attach: {:#}", anyhow::Error::from(error)),
            ));
            (None, None)
        }
    }
}
