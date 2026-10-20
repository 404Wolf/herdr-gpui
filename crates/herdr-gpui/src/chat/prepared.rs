//! What the view draws for an entry, prepared when the entry changes rather
//! than on every frame: agent text parsed as Markdown, and a tool call's
//! output and file changes summarized.

use crate::release_notes::{self, Line};
use herdr_chat_protocol::{EntryId, EntryKind, ToolContent, Transcript};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

/// Lines of a tool call's text output shown under it.
const TOOL_LINES: usize = 6;
/// Bytes of a diff's sides compared line by line; past them it is counted
/// as changed without its lines.
const DIFF_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffSummary {
    pub(crate) path: String,
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolSummary {
    pub(crate) diffs: Vec<DiffSummary>,
    /// The first lines of its text output.
    pub(crate) lines: Vec<String>,
    /// Text lines left out.
    pub(crate) more: usize,
    pub(crate) terminals: usize,
}

#[derive(Debug)]
pub(crate) enum Body {
    Markdown(Rc<[Line]>),
    Tool(ToolSummary),
}

/// Bodies by entry, each with the revision it was prepared at.
#[derive(Default)]
pub(crate) struct Prepared {
    bodies: HashMap<EntryId, (u64, Rc<Body>)>,
}

impl Prepared {
    pub(crate) fn body(&self, id: EntryId) -> Option<Rc<Body>> {
        self.bodies.get(&id).map(|(_, body)| body.clone())
    }

    /// Prepares the entries whose revision moved and forgets those gone.
    pub(crate) fn update(&mut self, transcript: &Transcript, keys: &[(EntryId, u64)]) {
        let live: HashSet<EntryId> = keys.iter().map(|(id, _)| *id).collect();
        self.bodies.retain(|id, _| live.contains(id));
        for (entry, (_, revision)) in transcript.entries.iter().zip(keys) {
            if self
                .bodies
                .get(&entry.id)
                .is_some_and(|(prepared, _)| prepared == revision)
            {
                continue;
            }
            let body = match &entry.kind {
                EntryKind::Agent { text } => Body::Markdown(release_notes::parse(text).into()),
                EntryKind::Tool(tool) => Body::Tool(summarize(&tool.content)),
                _ => continue,
            };
            self.bodies.insert(entry.id, (*revision, Rc::new(body)));
        }
    }

    /// Starts over, as after a snapshot that may reuse entry ids.
    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
    }
}

pub(crate) fn summarize(content: &[ToolContent]) -> ToolSummary {
    let mut summary = ToolSummary::default();
    for item in content {
        match item {
            ToolContent::Text { text } => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    if summary.lines.len() < TOOL_LINES {
                        summary
                            .lines
                            .push(crate::notifications::safe_text(line, 400));
                    } else {
                        summary.more += 1;
                    }
                }
            }
            ToolContent::Diff { new, old, path } => {
                let (added, removed) = diff_counts(old.as_deref().unwrap_or(""), new);
                summary.diffs.push(DiffSummary {
                    path: path.clone(),
                    added,
                    removed,
                });
            }
            ToolContent::Terminal { .. } => summary.terminals += 1,
        }
    }
    summary
}

/// Lines added and removed between two versions of a file, by counting
/// each distinct line on both sides: what `+`/`-` totals show, without the
/// cost of an alignment.
pub(crate) fn diff_counts(old: &str, new: &str) -> (usize, usize) {
    if old.len() + new.len() > DIFF_BYTES {
        return (new.lines().count(), old.lines().count());
    }
    let mut counts: HashMap<&str, isize> = HashMap::new();
    for line in old.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    for line in new.lines() {
        *counts.entry(line).or_default() += 1;
    }
    counts
        .values()
        .fold((0, 0), |(added, removed), &count| match count {
            count if count > 0 => (added + count.unsigned_abs(), removed),
            count => (added, removed + count.unsigned_abs()),
        })
}
