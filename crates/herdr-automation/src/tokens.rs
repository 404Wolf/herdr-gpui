//! Values a script can pass along but never read: the firing's date and
//! time, and what earlier steps produced. The script sees fixed tokens such
//! as `{date}` or `{log_tail:2}`, so its control flow cannot depend on them
//! and the plan shows every path it can take; the runner fills the tokens
//! in only when it hands a step to the host.
//!
//! Step output is untrusted, so it never becomes shell text: in a `sh`
//! command an output token becomes a quoted environment variable reference
//! and the value travels in the environment.

use crate::step::{Call, Step};

pub const DATE: &str = "{date}";
pub const TIME: &str = "{time}";

/// The token for step `index`'s output tail; steps count from 1 for people.
pub fn log_tail(index: usize) -> String {
    format!("{{log_tail:{}}}", index + 1)
}

/// The token for the agent run step `index` started.
pub fn run_id(index: usize) -> String {
    format!("{{run_id:{}}}", index + 1)
}

/// The real values behind the tokens, for one run.
#[derive(Clone, Copy, Debug)]
pub struct Values<'a> {
    pub date: &'a str,
    pub time: &'a str,
    /// Indexed by step; a missing or `None` entry leaves its token as is.
    pub log_tails: &'a [Option<String>],
    pub run_ids: &'a [Option<String>],
}

impl Values<'_> {
    /// `text` with every known token replaced. Unknown braces stay as they
    /// are, so a script cannot break a step by writing `{` itself.
    pub fn fill(&self, text: &str) -> String {
        self.replace(text, false, &mut Vec::new())
    }

    /// A shell command with output tokens turned into `"$HERDR_..."`
    /// references; returns the variables to set. Date and time are digits,
    /// dashes and colons, so they are written in directly.
    pub fn fill_shell(&self, text: &str) -> (String, Vec<(String, String)>) {
        let mut env = Vec::new();
        let command = self.replace(text, true, &mut env);
        (command, env)
    }

    fn replace(&self, text: &str, shell: bool, env: &mut Vec<(String, String)>) -> String {
        let mut filled = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            filled.push_str(&rest[..open]);
            let tail = &rest[open..];
            let Some(close) = tail.find('}') else {
                filled.push_str(tail);
                return filled;
            };
            let token = &tail[..=close];
            match self.value(token) {
                Some(value) if shell && token != DATE && token != TIME => {
                    let name = variable(token);
                    filled.push_str(&format!("\"${name}\""));
                    if !env.iter().any(|(known, _)| *known == name) {
                        env.push((name, value.to_owned()));
                    }
                }
                Some(value) => filled.push_str(value),
                None => filled.push_str(token),
            }
            rest = &tail[close + 1..];
        }
        filled.push_str(rest);
        filled
    }

    fn value(&self, token: &str) -> Option<&str> {
        match token {
            DATE => return Some(self.date),
            TIME => return Some(self.time),
            _ => {}
        }
        let inner = token.strip_prefix('{')?.strip_suffix('}')?;
        let (name, number) = inner.split_once(':')?;
        let index = number.parse::<usize>().ok()?.checked_sub(1)?;
        let list = match name {
            "log_tail" => self.log_tails,
            "run_id" => self.run_ids,
            _ => return None,
        };
        list.get(index)?.as_deref()
    }
}

/// `{log_tail:2}` as `HERDR_LOG_TAIL_2`.
fn variable(token: &str) -> String {
    let inner = token.trim_start_matches('{').trim_end_matches('}');
    format!("HERDR_{}", inner.replace(':', "_").to_ascii_uppercase())
}

/// A call with its tokens filled in, ready to hand to the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub call: Call,
    /// Variables a `sh` command reads its output tokens from.
    pub env: Vec<(String, String)>,
}

impl Call {
    pub fn resolve(&self, values: &Values<'_>) -> Resolved {
        let mut env = Vec::new();
        let fill = |text: &String| values.fill(text);
        let fill_opt = |text: &Option<String>| text.as_ref().map(fill);
        let step = match &self.step {
            Step::Agent {
                harness,
                prompt,
                worktree,
                base,
                wait,
            } => Step::Agent {
                harness: fill(harness),
                prompt: fill(prompt),
                worktree: fill_opt(worktree),
                base: fill_opt(base),
                wait: *wait,
            },
            Step::Notify { text } => Step::Notify { text: fill(text) },
            Step::Pull { branch } => Step::Pull {
                branch: fill(branch),
            },
            Step::Run { name } => Step::Run { name: name.clone() },
            Step::Sh { command } => {
                let (command, variables) = values.fill_shell(command);
                env = variables;
                Step::Sh { command }
            }
            Step::Worktree { name, base } => Step::Worktree {
                name: fill(name),
                base: fill_opt(base),
            },
        };
        Resolved {
            call: Self {
                step,
                check: self.check,
                timeout: self.timeout,
            },
            env,
        }
    }
}
