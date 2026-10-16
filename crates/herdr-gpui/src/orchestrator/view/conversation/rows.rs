//! The conversation as list rows. Each review heading, card bar, and body
//! line is a row of its own, so the list lays out and paints only the rows in
//! view, the way a terminal draws only its visible lines; a card is drawn
//! across its rows, each carrying its share of the border. Rows are rebuilt
//! only when what they show changes, and the list keeps its scroll position
//! across a rebuild by replacing only the rows that differ.

use super::{Entry, Summary, entries, summary, thread_key};
use crate::{
    orchestrator::view::look::{GREEN, MAGENTA, RED},
    pr_actions::{Comment, CommentKind, Review},
    release_notes::{self, Line},
};
use gpui::{ListAlignment, ListState, Pixels, px};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

/// Space between the blocks of one entry, and between entries.
const BLOCK_GAP: f32 = 8.;
const ENTRY_GAP: f32 = 12.;
/// Parsed bodies kept before the cache starts over.
const BODY_LIMIT: usize = 128;
/// Resolved threads unfolded before they all fold again.
const SHOWN_LIMIT: usize = 256;
/// How far past the viewport the list lays rows out ahead of a scroll.
const OVERDRAW: f32 = 400.;
/// The height the scrollbar counts for a row the list has not measured yet:
/// about one body line, the row a conversation holds most of.
const ROW_HINT: f32 = 20.;

/// One row of the conversation list.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) piece: Piece,
    pub(super) gutter: Gutter,
    pub(super) card: Option<Card>,
    /// Space above the row. Above rather than below, so a comment arriving
    /// at the end leaves the rows before it as they were.
    pub(super) gap: f32,
}

/// The avatar column beside a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Gutter {
    /// None: the row spans the width.
    Wide,
    /// The entry's avatar shows on this row, its first.
    Avatar,
    /// Under the avatar, past the entry's first row.
    Blank,
}

/// The card a row is a slice of: its border tint, and whether the row is
/// the card's top or bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Card {
    pub(super) tint: Option<usize>,
    pub(super) top: bool,
    pub(super) bottom: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Piece {
    Summary(Summary),
    /// Muted text: why no comments show, that there are none, or the
    /// newest action's outcome.
    Notice(String),
    /// The newest action's failure.
    Failure(String),
    /// A conversation comment's bar: author, time.
    Commented(Rc<Comment>),
    /// A review's heading, from its lead comment: the review itself or its
    /// first thread when its author left no review.
    Reviewed {
        lead: Rc<Comment>,
        verdict: Option<(&'static str, usize)>,
        threads: usize,
    },
    /// A resolved thread folded to a line.
    Folded {
        thread: Rc<Comment>,
        key: String,
    },
    /// An inline thread's bar: its file, replies, and state.
    Thread(Rc<Comment>),
    /// Line `line` of a body.
    Body {
        lines: Rc<[Line]>,
        line: usize,
    },
    /// A body with nothing to show.
    Empty,
    Composer,
}

/// What the conversation shows besides its comments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Chrome {
    /// Why no comments show: signed out, a failed read, or still reading.
    pub(super) notice: Option<String>,
    pub(super) outcome: Option<String>,
    pub(super) failure: Option<String>,
    pub(super) composer: bool,
}

/// The conversation's list, its rows, and what they were built from.
pub(crate) struct Rows {
    list: ListState,
    rows: Vec<Row>,
    /// The comments and chrome the rows show; `None` until built, and again
    /// after a thread unfolds. Comments compare by pointer: a re-read is a
    /// new `Arc`, so scrolling never reads the text to find nothing changed.
    built: Option<(Option<Arc<[Comment]>>, Chrome)>,
    shown: HashSet<String>,
    bodies: HashMap<String, Rc<[Line]>>,
    /// Where the pointer holds the scrollbar's thumb, from its top.
    pub(super) grab: f32,
    /// The list width unmeasured rows were last given `ROW_HINT` at.
    hinted: Cell<Option<Pixels>>,
}

impl Default for Rows {
    fn default() -> Self {
        Self {
            list: ListState::new(0, ListAlignment::Top, px(OVERDRAW)),
            rows: Vec::new(),
            built: None,
            shown: HashSet::new(),
            bodies: HashMap::new(),
            grab: 0.,
            hinted: Cell::new(None),
        }
    }
}

impl Rows {
    pub(crate) fn list(&self) -> ListState {
        self.list.clone()
    }

    pub(super) fn get(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    /// Row heights change with the font; the count and order do not.
    pub(crate) fn remeasure(&self) {
        self.list.remeasure();
    }

    /// Unfolds a resolved thread.
    pub(super) fn show(&mut self, key: String) {
        if self.shown.len() >= SHOWN_LIMIT {
            self.shown.clear();
        }
        self.shown.insert(key);
        self.built = None;
    }

    /// Gives the rows the list has not measured `ROW_HINT`, so the thumb is
    /// near its size before they lay out. New rows and a width change, which
    /// makes the list forget every height, need it again.
    pub(super) fn hint(&self) {
        let width = self.list.viewport_bounds().size.width;
        if self.hinted.get() != Some(width) {
            self.hinted.set(Some(width));
            self.list.clone().with_uniform_item_height(px(ROW_HINT));
        }
    }

    /// Rebuilds the rows when `comments` or `chrome` differ from what they
    /// show, telling the list only which rows changed.
    pub(super) fn update(&mut self, comments: Option<&Arc<[Comment]>>, chrome: Chrome) {
        if let Some((built, built_chrome)) = &self.built
            && match (built, comments) {
                (Some(built), Some(comments)) => Arc::ptr_eq(built, comments),
                (None, None) => true,
                _ => false,
            }
            && *built_chrome == chrome
        {
            return;
        }
        let rows = self.build(comments.map(|comments| &**comments), &chrome);
        for (range, count) in changes(&self.rows, &rows) {
            self.list.splice(range, count);
        }
        self.hinted.set(None);
        self.rows = rows;
        self.built = Some((comments.cloned(), chrome));
    }

    fn build(&mut self, comments: Option<&[Comment]>, chrome: &Chrome) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(notice) = &chrome.notice {
            rows.push(wide(Piece::Notice(notice.clone())));
        }
        if let Some(comments) = comments {
            rows.push(wide(if comments.is_empty() {
                Piece::Notice("No comments yet.".into())
            } else {
                Piece::Summary(summary(comments))
            }));
            for entry in entries(comments) {
                match entry {
                    Entry::Comment(comment) => {
                        rows.push(Row {
                            piece: Piece::Commented(Rc::new(comment.clone())),
                            gutter: Gutter::Avatar,
                            card: Some(Card {
                                tint: None,
                                top: true,
                                bottom: false,
                            }),
                            gap: ENTRY_GAP,
                        });
                        self.body(&mut rows, comment, None, false);
                    }
                    Entry::Review {
                        review, threads, ..
                    } => self.review(&mut rows, review, &threads),
                }
            }
        }
        let tail = [
            chrome.outcome.clone().map(Piece::Notice),
            chrome.failure.clone().map(Piece::Failure),
            chrome.composer.then_some(Piece::Composer),
        ];
        rows.extend(tail.into_iter().flatten().map(wide));
        if let Some(first) = rows.first_mut() {
            first.gap = 0.;
        }
        rows
    }

    fn review(&mut self, rows: &mut Vec<Row>, review: Option<&Comment>, threads: &[&Comment]) {
        let Some(lead) = review.or(threads.first().copied()) else {
            return;
        };
        let verdict = review.and_then(|review| match review.kind {
            CommentKind::Review(Review::Approved) => Some(("approved", GREEN)),
            CommentKind::Review(Review::ChangesRequested) => Some(("changes requested", RED)),
            CommentKind::Review(Review::Dismissed) => Some(("dismissed", MAGENTA)),
            _ => None,
        });
        rows.push(Row {
            piece: Piece::Reviewed {
                lead: Rc::new(lead.clone()),
                verdict,
                threads: threads.len(),
            },
            gutter: Gutter::Avatar,
            card: None,
            gap: ENTRY_GAP,
        });
        if let Some(review) = review.filter(|review| !review.source.trim().is_empty()) {
            self.body(rows, review, verdict.map(|(_, hue)| hue), true);
        }
        for thread in threads {
            let resolved = matches!(thread.kind, CommentKind::Thread { resolved: true, .. });
            let key = thread_key(thread);
            if resolved && !self.shown.contains(&key) {
                rows.push(Row {
                    piece: Piece::Folded {
                        thread: Rc::new((*thread).clone()),
                        key,
                    },
                    gutter: Gutter::Blank,
                    card: None,
                    gap: BLOCK_GAP,
                });
                continue;
            }
            rows.push(Row {
                piece: Piece::Thread(Rc::new((*thread).clone())),
                gutter: Gutter::Blank,
                card: Some(Card {
                    tint: None,
                    top: true,
                    bottom: false,
                }),
                gap: BLOCK_GAP,
            });
            self.body(rows, thread, None, false);
        }
    }

    /// The rows of `comment`'s body: the bottom of its card, or a card of
    /// its own, a block apart from the heading, when `alone`.
    fn body(&mut self, rows: &mut Vec<Row>, comment: &Comment, tint: Option<usize>, alone: bool) {
        let lines = self.lines(&comment.source);
        let pieces: Vec<Piece> = if lines.is_empty() {
            vec![Piece::Empty]
        } else {
            (0..lines.len())
                .map(|line| Piece::Body {
                    lines: lines.clone(),
                    line,
                })
                .collect()
        };
        let last = pieces.len() - 1;
        rows.extend(pieces.into_iter().enumerate().map(|(index, piece)| Row {
            piece,
            gutter: Gutter::Blank,
            card: Some(Card {
                tint,
                top: alone && index == 0,
                bottom: index == last,
            }),
            gap: if alone && index == 0 { BLOCK_GAP } else { 0. },
        }));
    }

    fn lines(&mut self, source: &str) -> Rc<[Line]> {
        if source.trim().is_empty() {
            return Rc::from([]);
        }
        if let Some(lines) = self.bodies.get(source) {
            return lines.clone();
        }
        if self.bodies.len() >= BODY_LIMIT {
            self.bodies.clear();
        }
        let lines: Rc<[Line]> =
            release_notes::parse(&crate::orchestrator::view::markdown::without_html(source)).into();
        self.bodies.insert(source.to_owned(), lines.clone());
        lines
    }
}

fn wide(piece: Piece) -> Row {
    Row {
        piece,
        gutter: Gutter::Wide,
        card: None,
        gap: ENTRY_GAP,
    }
}

/// The splices that turn `old` into `new`, in order. A row replaced in
/// place between matching rows, such as the summary when a comment arrives,
/// is a splice of its own, so the list keeps its scroll position past it;
/// the rest is everything between the rows both start and end with.
fn changes(old: &[Row], new: &[Row]) -> Vec<(Range<usize>, usize)> {
    let mut splices = Vec::new();
    let mut start = 0;
    while start < old.len() && start < new.len() {
        if old[start] != new[start] {
            let next = old.get(start + 1);
            if next.is_none() || next != new.get(start + 1) {
                break;
            }
            splices.push((start..start + 1, 1));
        }
        start += 1;
    }
    if start == old.len() && start == new.len() {
        return splices;
    }
    let end = old[start..]
        .iter()
        .rev()
        .zip(new[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    splices.push((start..old.len() - end, new.len() - start - end));
    splices
}
