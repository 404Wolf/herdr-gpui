//! Rounder bubbles with more room around them, and a composer that is one
//! rounded field: the attach button leads, the draft fills it, and the
//! settings and send button close it, as ChatGPT's and Codex's apps draw
//! theirs. A narrow pane wraps the settings under the draft.

use super::{Bubble, ChatLayout, ComposerParts, Look, Row, bubble_row, summary_row};
use crate::config::mix;
use gpui::{prelude::*, *};

const SHAPE: Bubble = Bubble {
    radius: 18.,
    padding: 9.,
    gap: 24.,
};
/// The field's corner radius: a pill while the draft is one line.
const RADIUS: f32 = 24.;
/// The least room the draft keeps before the settings wrap under it.
const DRAFT: f32 = 180.;

pub(super) struct Pill;

impl ChatLayout for Pill {
    fn row(&self, row: Row<'_>, look: &Look<'_>) -> AnyElement {
        if row.side().is_none() {
            return summary_row(row, SHAPE.gap);
        }
        bubble_row(row, look, SHAPE)
    }

    fn composer(&self, parts: ComposerParts, look: &Look<'_>) -> Div {
        let theme = look.theme;
        let field = div()
            .flex()
            .flex_col()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(rgb(mix(theme.background, theme.foreground, 16)))
            .bg(rgb(mix(theme.background, theme.foreground, 7)))
            .children(parts.chips.map(|chips| chips.px(px(14.))))
            .child(
                div()
                    .debug_selector(|| "chat-settings".into())
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(7.))
                    .child(parts.attach)
                    .child(parts.draft.flex_1().min_w(px(DRAFT)).px(px(4.)).py(px(2.)))
                    .children(parts.settings)
                    .child(parts.send),
            );
        div().flex().flex_col().gap(px(4.)).child(field).children(
            parts
                .usage
                .map(|usage| div().flex().justify_end().px(px(RADIUS / 2.)).child(usage)),
        )
    }
}
