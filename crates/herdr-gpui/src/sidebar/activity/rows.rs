//! What the list panels show, built from data the window already holds: the
//! open orchestrator views' items and runs, and the PR prefetch's answers for
//! the shown device's checkouts. Pure, so it is tested without a window.

use crate::{
    browser::TabId,
    orchestrator::{Item, Run, RunStatus},
    pull_request::{Outcome, PullRequest, State},
};
use std::collections::HashSet;

/// Rows kept per section: far more than a sidebar shows, bounded for a huge
/// repository.
pub(super) const ROW_LIMIT: usize = 200;

/// What a row's tag says, which picks its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    /// Waiting on the user: a failed check, an agent asking.
    Attention,
    /// Under way: checks or an agent running.
    Busy,
    /// Done well: checks passed, merged.
    Good,
    /// Neither good nor bad news: queued, draft, closed.
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Tag {
    pub(super) label: &'static str,
    pub(super) tone: Tone,
}

impl Tag {
    const fn new(label: &'static str, tone: Tone) -> Self {
        Self { label, tone }
    }
}

/// What clicking a row opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Opens {
    /// An orchestrator item, in the view of orchestrator tab `tab`.
    Item { tab: TabId, key: String },
    /// An orchestrator run, in the view of orchestrator tab `tab`.
    Run { tab: TabId, id: String },
    /// A workspace, with the menu that carries its pull request's card.
    Workspace(String),
}

/// A two-line row: `#612 title` over `repository · detail`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) number: String,
    pub(super) title: String,
    pub(super) meta: String,
    pub(super) tag: Option<Tag>,
    pub(super) opens: Opens,
}

impl Row {
    /// A case-insensitive substring of any of the row's text; `query` is
    /// already lowercase.
    fn matches(&self, query: &str) -> bool {
        [&self.number, &self.title, &self.meta]
            .into_iter()
            .any(|text| text.to_lowercase().contains(query))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) title: &'static str,
    pub(super) rows: Vec<Row>,
}

/// One open orchestrator view's lists.
#[derive(Clone, Copy)]
pub(super) struct Source<'a> {
    pub(super) tab: TabId,
    pub(super) items: &'a [Item],
    pub(super) runs: &'a [Run],
    /// Each run's status as the view shows it, live agents considered.
    pub(super) statuses: &'a [RunStatus],
}

impl<'a> Source<'a> {
    fn runs(&self) -> impl Iterator<Item = (&'a Run, RunStatus)> + use<'a> {
        self.runs.iter().zip(self.statuses.iter().copied())
    }
}

/// A pull request the prefetch found for one of the shown device's
/// workspaces.
#[derive(Clone, Copy)]
pub(super) struct BranchPr<'a> {
    pub(super) workspace: &'a str,
    pub(super) pr: &'a PullRequest,
}

/// Keeps the rows `query` matches and drops the sections left empty. A blank
/// query keeps everything.
pub(super) fn filter(sections: Vec<Section>, query: &str) -> Vec<Section> {
    let query = query.trim().to_lowercase();
    sections
        .into_iter()
        .map(|mut section| {
            if !query.is_empty() {
                section.rows.retain(|row| row.matches(&query));
            }
            section
        })
        .filter(|section| !section.rows.is_empty())
        .collect()
}

pub(super) fn count(sections: &[Section]) -> usize {
    sections.iter().map(|section| section.rows.len()).sum()
}

/// Runs, grouped as the orchestrator's Runs tab groups them: what needs the
/// user first, then working, idle, and finished.
pub(super) fn orchestrator(sources: &[Source]) -> Vec<Section> {
    let mut sections = [
        ("Needs you", Vec::new()),
        ("Working", Vec::new()),
        ("Idle", Vec::new()),
        ("Finished", Vec::new()),
    ];
    for source in sources {
        for (run, status) in source.runs() {
            let place = match status {
                RunStatus::NeedsInput | RunStatus::Failed => 0,
                RunStatus::Starting | RunStatus::Working => 1,
                RunStatus::Idle => 2,
                RunStatus::Done | RunStatus::Stopped => 3,
            };
            let rows = &mut sections[place].1;
            if rows.len() < ROW_LIMIT {
                rows.push(run_row(*source, run, status));
            }
        }
    }
    sections
        .into_iter()
        .map(|(title, rows)| Section { title, rows })
        .collect()
}

/// How many runs need the user or are working, for the bar's count.
pub(super) fn active_runs(sources: &[Source]) -> usize {
    sources
        .iter()
        .flat_map(Source::runs)
        .filter(|(_, status)| active(*status))
        .count()
}

/// Whether an agent is on the run or waiting for the user on it.
fn active(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Starting | RunStatus::Working | RunStatus::NeedsInput | RunStatus::Failed
    )
}

fn run_row(source: Source, run: &Run, status: RunStatus) -> Row {
    let item = source
        .items
        .iter()
        .find(|item| item.key.canonical() == run.item_key);
    let repository = item.map_or("", |item| repository_name(&item.key.source.repository));
    Row {
        number: item.map(|item| item.identifier.clone()).unwrap_or_default(),
        title: item.map_or_else(|| run.agent.clone(), |item| item.title.clone()),
        meta: join(&[repository, &run.agent]),
        tag: Some(run_tag(status)),
        opens: Opens::Run {
            tab: source.tab,
            id: run.id.clone(),
        },
    }
}

/// The Runs tab's own words for a status.
fn run_tag(status: RunStatus) -> Tag {
    let tone = match status {
        RunStatus::NeedsInput | RunStatus::Failed => Tone::Attention,
        RunStatus::Starting | RunStatus::Working => Tone::Busy,
        RunStatus::Done => Tone::Good,
        RunStatus::Idle | RunStatus::Stopped => Tone::Quiet,
    };
    Tag::new(status.label(), tone)
}

/// Open issues, with a tag on those an agent works on.
pub(super) fn issues(sources: &[Source]) -> Vec<Section> {
    let mut rows = Vec::new();
    for source in sources {
        let working: HashSet<&str> = source
            .runs()
            .filter(|(_, status)| active(*status) || *status == RunStatus::Idle)
            .map(|(run, _)| run.item_key.as_str())
            .collect();
        for item in source.items {
            if item.pull_request.is_some() || !open(item) || rows.len() >= ROW_LIMIT {
                continue;
            }
            let key = item.key.canonical();
            let tag = working
                .contains(key.as_str())
                .then_some(Tag::new("agent", Tone::Busy));
            rows.push(item_row(source.tab, item, key, tag));
        }
    }
    vec![Section {
        title: "Open",
        rows,
    }]
}

/// The shown device's branches' pull requests first, then the open ones the
/// orchestrator lists that no branch here has.
pub(super) fn pull_requests(branches: &[BranchPr], sources: &[Source]) -> Vec<Section> {
    let mut seen = HashSet::new();
    let mut yours = Vec::new();
    for branch in branches {
        let pr = branch.pr;
        let repository = url_repository(&pr.url);
        // Two worktrees on one branch share its pull request.
        if yours.len() >= ROW_LIMIT || !seen.insert((repository, pr.number)) {
            continue;
        }
        yours.push(Row {
            number: format!("#{}", pr.number),
            title: pr.title.clone(),
            meta: join(&[repository_name(repository), &pr.head_ref_name]),
            tag: pr_tag(pr),
            opens: Opens::Workspace(branch.workspace.to_owned()),
        });
    }
    let mut listed = Vec::new();
    for source in sources {
        for item in source.items {
            let Some(pr) = &item.pull_request else {
                continue;
            };
            let repository = item.key.source.repository.as_str();
            if listed.len() >= ROW_LIMIT || !open(item) || !seen.insert((repository, pr.number)) {
                continue;
            }
            listed.push(item_row(source.tab, item, item.key.canonical(), None));
        }
    }
    vec![
        Section {
            title: "Your branches",
            rows: yours,
        },
        Section {
            title: "Repository",
            rows: listed,
        },
    ]
}

/// The tag a branch's pull request shows: settled state first, then a draft,
/// then its checks.
fn pr_tag(pr: &PullRequest) -> Option<Tag> {
    match pr.state {
        State::Merged => return Some(Tag::new("merged", Tone::Good)),
        State::Closed => return Some(Tag::new("closed", Tone::Quiet)),
        State::Open | State::Unknown => {}
    }
    if pr.is_draft {
        return Some(Tag::new("draft", Tone::Quiet));
    }
    pr.checks_outcome().map(|outcome| match outcome {
        Outcome::Failed => Tag::new("CI failed", Tone::Attention),
        Outcome::Pending => Tag::new("CI running", Tone::Busy),
        Outcome::Passed | Outcome::Skipped => Tag::new("CI passing", Tone::Good),
    })
}

fn item_row(tab: TabId, item: &Item, key: String, tag: Option<Tag>) -> Row {
    let label = item.labels.first().map_or("", String::as_str);
    Row {
        number: item.identifier.clone(),
        title: item.title.clone(),
        meta: join(&[repository_name(&item.key.source.repository), label]),
        tag,
        opens: Opens::Item { tab, key },
    }
}

/// Whether the provider still counts the item as open. Beads' own words for
/// unfinished work count too.
fn open(item: &Item) -> bool {
    !matches!(item.state.as_str(), "closed" | "merged" | "done")
}

/// `owner/name` from a pull request's `https://host/owner/name/pull/N`, the
/// form the orchestrator keys GitHub repositories by.
fn url_repository(url: &str) -> &str {
    let path = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = path.split_once('/').map_or("", |(_, path)| path);
    path.split_once("/pull/")
        .map_or(path, |(repository, _)| repository)
}

/// `owner/name` as `name`, which is what fits beside a detail.
fn repository_name(repository: &str) -> &str {
    let trimmed = repository.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ")
}
