//! Following a streaming reply by gliding to the end of the chat instead of
//! jumping there with each chunk.
//!
//! The list itself never follows its tail: aligned to its top, it keeps
//! its top where it is while entries grow beneath it, even at the end,
//! where a list aligned to its bottom would stick to the end and jump.
//! Each frame [`Glide`] scrolls part of the way to the end, easing out,
//! until it is there, and [`Glide::watch`] asks for that frame once a
//! layout leaves the list behind its end, whether an entry grew or the
//! list itself got shorter. Scrolling up stops the following; scrolling
//! back to the end starts it again.

use gpui::{ListOffset, ListState, canvas, prelude::*, px};
use std::{cell::Cell, ops::Range, rc::Rc, time::Instant};

/// The time constant of the ease: about two thirds of what is left is
/// covered in this long, so a few new lines settle in about 120ms and the
/// view keeps up with text as it streams.
const EASE_SECONDS: f32 = 0.04;
/// The longest frame gap counted, so a stall does not land in one jump.
const MAX_FRAME_SECONDS: f32 = 0.05;
/// A frame gap assumed for the first step of a glide.
const FRAME_SECONDS: f32 = 1. / 60.;
/// Distances below this count as being at the end.
const AT_END: f32 = 1.;
/// The least a step moves, so the end comes in finite time.
const MIN_STEP: f32 = 0.5;
/// More than this many viewports behind is closed at once, down to it, so
/// a long burst of output does not take seconds to scroll past.
const MAX_BEHIND: f32 = 0.5;

#[derive(Debug)]
pub(crate) struct Glide {
    /// Shared with the frame's [`Self::watch`], which reads it after layout.
    following: Rc<Cell<bool>>,
    /// Set by the list when the user scrolls it, by wheel or trackpad. Only
    /// the user's scrolling decides following: the list moving its own top,
    /// as it does when the end comes up short, must not stop it.
    scrolled: Rc<Cell<bool>>,
    last_step: Option<Instant>,
}

impl Glide {
    /// Follows `list`'s end, watching it for the user's scrolling.
    pub(crate) fn new(list: &ListState) -> Self {
        let scrolled = Rc::new(Cell::new(false));
        list.set_scroll_handler({
            let scrolled = scrolled.clone();
            move |_, _, _| scrolled.set(true)
        });
        Self {
            following: Rc::new(Cell::new(true)),
            scrolled,
            last_step: None,
        }
    }

    /// Back to following, with `list`, which starts over, at its end.
    pub(crate) fn reset(&mut self, list: &ListState) {
        self.following.set(true);
        self.scrolled.set(false);
        self.last_step = None;
        list.scroll_to(ListOffset {
            item_ix: list.item_count(),
            offset_in_item: px(0.),
        });
    }

    /// Takes a scroll a test made on the list as the user's own: it stops
    /// the following unless it ends at the end. The user's wheel and
    /// trackpad tell the list directly.
    #[cfg(test)]
    pub(crate) fn user_scrolled(&self) {
        self.scrolled.set(true);
    }

    /// An empty element to place after `list` in the frame: once the list
    /// is laid out, with heights the frame's render could not know yet, it
    /// asks for another frame if the list is behind its end and following.
    pub(crate) fn watch(&self, list: &ListState) -> impl IntoElement + use<> {
        let following = self.following.clone();
        let list = list.clone();
        canvas(
            move |_, window, _| {
                if following.get() && behind(&list) > MIN_STEP {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// Scrolls `list` one frame's step toward its end, and says whether to
    /// draw another frame.
    pub(crate) fn tick(&mut self, list: &ListState, now: Instant) -> bool {
        let top = -f32::from(list.scroll_px_offset_for_scrollbar().y);
        let end = top + behind(list);
        let viewport = f32::from(list.viewport_bounds().size.height);
        if let Some(distance) = self.step(top, end, viewport, now) {
            list.scroll_by(px(distance));
        }
        self.last_step.is_some()
    }

    /// How far to scroll this frame, from the top's offset, the offset at
    /// the end, and the viewport's height, all in pixels.
    fn step(&mut self, top: f32, end: f32, viewport: f32, now: Instant) -> Option<f32> {
        let remaining = end - top;
        // The user's scroll leaves the list following only at its end.
        if self.scrolled.take() {
            self.following.set(remaining <= AT_END);
        }
        if !self.following.get() || remaining <= MIN_STEP {
            self.last_step = None;
            return None;
        }
        let elapsed = self.last_step.map_or(FRAME_SECONDS, |last| {
            now.saturating_duration_since(last)
                .as_secs_f32()
                .min(MAX_FRAME_SECONDS)
        });
        self.last_step = Some(now);
        let jump = (remaining - viewport * MAX_BEHIND).max(0.);
        let left = remaining - jump;
        let ease = left * (1. - (-elapsed / EASE_SECONDS).exp());
        Some((jump + ease.max(MIN_STEP)).min(remaining))
    }
}

/// How far `list`'s top is above its end, in pixels, as of its last layout.
fn behind(list: &ListState) -> f32 {
    f32::from(list.max_offset_for_scrollbar().y + list.scroll_px_offset_for_scrollbar().y)
}

/// `list.splice`, keeping the list's top where it was when the entry it
/// falls in is redrawn in place, such as a reply that grew: the list
/// itself moves the top to that entry's start.
pub(crate) fn splice_in_place(list: &ListState, range: Range<usize>, count: usize) {
    let top = list.logical_scroll_top();
    list.splice(range.clone(), count);
    if range.len() == count && range.contains(&top.item_ix) {
        list.scroll_to(top);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
