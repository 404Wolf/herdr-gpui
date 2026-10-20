//! A bounded multiline composer. UTF-8 offsets stay internal; the native IME
//! boundary speaks UTF-16. Shaping supplies both hit testing and IME geometry.
use crate::{
    config::{FontConfig, Theme},
    input::ViewInputHandler,
};
use gpui::{prelude::*, *};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Submit;
pub(super) struct Composer {
    pub focus: FocusHandle,
    edit: Edit,
    font: FontConfig,
    theme: Theme,
    lines: Vec<WrappedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll: Pixels,
    reveal: bool,
    dragging: bool,
}

#[derive(Clone, Default)]
struct Edit {
    text: String,
    anchor: usize,
    cursor: usize,
    marked: Option<Range<usize>>,
}

fn byte(text: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, ch) in text.char_indices() {
        if n >= utf16 {
            return i;
        }
        n += ch.len_utf16();
    }
    text.len()
}
fn units(text: &str, index: usize) -> usize {
    text[..index].encode_utf16().count()
}
fn range(text: &str, asked: Range<usize>) -> Range<usize> {
    byte(text, asked.start.min(asked.end))..byte(text, asked.start.max(asked.end))
}

impl Edit {
    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }
    fn move_to(&mut self, at: usize, extend: bool) {
        self.cursor = at;
        if !extend {
            self.anchor = at;
        }
    }
    fn replace(
        &mut self,
        asked: Option<Range<usize>>,
        text: &str,
        marked: bool,
        selected: Option<Range<usize>>,
    ) {
        let selected_range = asked
            .map(|r| range(&self.text, r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let text: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        if self.text.len() - selected_range.len() + text.len()
            > herdr_client::chat::MAX_PROMPT_BYTES
        {
            return;
        }
        self.text.replace_range(selected_range.clone(), &text);
        let end = selected_range.start + text.len();
        let selected = selected.map(|r| range(&text, r));
        self.anchor = selected
            .as_ref()
            .map_or(end, |r| selected_range.start + r.start);
        self.cursor = selected
            .as_ref()
            .map_or(end, |r| selected_range.start + r.end);
        self.marked = (marked && !text.is_empty()).then_some(selected_range.start..end);
    }
}

impl Composer {
    pub fn new(font: FontConfig, theme: Theme, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            edit: Edit::default(),
            font,
            theme,
            lines: Vec::new(),
            bounds: None,
            scroll: px(0.),
            reveal: true,
            dragging: false,
        }
    }
    pub fn text(&self) -> &str {
        &self.edit.text
    }
    pub fn composing(&self) -> bool {
        self.edit.marked.is_some()
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.edit = Edit::default();
        self.reveal = true;
        cx.notify();
    }

    pub fn restore(&mut self, text: &str, cx: &mut Context<Self>) {
        self.edit = Edit::default();
        self.edit.replace(None, text, false, None);
        self.reveal = true;
        cx.notify();
    }
    pub fn appearance(&mut self, font: FontConfig, theme: Theme, cx: &mut Context<Self>) {
        if self.font != font || self.theme != theme {
            self.font = font;
            self.theme = theme;
            cx.notify();
        }
    }
    fn position(&self, index: usize) -> Point<Pixels> {
        let mut start = 0;
        let mut y = px(0.);
        for line in &self.lines {
            if index <= start + line.len() {
                return point(px(0.), y)
                    + line
                        .position_for_index(index - start, px(self.font.line_height()))
                        .unwrap_or_default();
            }
            start += line.len() + 1;
            y += line.size(px(self.font.line_height())).height;
        }
        point(px(0.), y)
    }
    fn index(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.bounds else {
            return self.edit.cursor;
        };
        let mut position = position - bounds.origin + point(px(0.), self.scroll);
        let mut start = 0;
        for line in &self.lines {
            let height = line.size(px(self.font.line_height())).height;
            if position.y < height {
                let index = line
                    .closest_index_for_position(position, px(self.font.line_height()))
                    .unwrap_or_else(|i| i);
                return (start + index).min(self.edit.text.len());
            }
            start += line.len() + 1;
            position.y -= height;
        }
        self.edit.text.len()
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing() {
            return;
        }
        let key = &event.keystroke;
        let edit_mod = crate::actions::edit_modifiers(key.modifiers);
        if key.key == "enter" && edit_mod {
            cx.emit(Submit);
            cx.stop_propagation();
            return;
        }
        let previous = self
            .edit
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .rfind(|i| *i < self.edit.cursor)
            .unwrap_or(0);
        let next = self
            .edit
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > self.edit.cursor)
            .unwrap_or(self.edit.text.len());
        match key.key.as_str() {
            "a" if edit_mod => {
                self.edit.anchor = 0;
                self.edit.cursor = self.edit.text.len();
            }
            "c" | "x" if edit_mod => {
                let selection = self.edit.selection();
                if !selection.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.edit.text[selection].into(),
                    ));
                    if key.key == "x" {
                        self.edit.replace(None, "", false, None);
                    }
                }
            }
            "v" if edit_mod => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.edit.replace(None, &text, false, None);
                }
            }
            "enter" if !key.modifiers.control && !key.modifiers.platform && !key.modifiers.alt => {
                self.edit.replace(None, "\n", false, None)
            }
            "left" | "right" => {
                let backward = key.key == "left";
                let at = if crate::actions::word_modifiers(key.modifiers) {
                    if backward {
                        crate::words::previous_start(&self.edit.text, self.edit.cursor)
                    } else {
                        crate::words::next_end(&self.edit.text, self.edit.cursor)
                    }
                } else if key.modifiers.platform {
                    if backward { 0 } else { self.edit.text.len() }
                } else if backward {
                    previous
                } else {
                    next
                };
                self.edit.move_to(at, key.modifiers.shift);
            }
            "up" | "down" => {
                if let Some(bounds) = self.bounds {
                    let point = self.position(self.edit.cursor) + bounds.origin
                        - point(px(0.), self.scroll)
                        + point(
                            px(0.),
                            px(self.font.line_height()) * if key.key == "up" { -1. } else { 1. },
                        );
                    self.edit.move_to(self.index(point), key.modifiers.shift);
                }
            }
            "home" | "end" => self.edit.move_to(
                if key.key == "home" {
                    0
                } else {
                    self.edit.text.len()
                },
                key.modifiers.shift,
            ),
            "backspace" | "delete" => {
                if self.edit.selection().is_empty() {
                    self.edit.anchor = if key.key == "backspace" {
                        previous
                    } else {
                        next
                    };
                }
                self.edit.replace(None, "", false, None);
            }
            "escape" => {
                window.blur(cx);
            }
            _ => return,
        }
        self.reveal = true;
        cx.stop_propagation();
        cx.notify();
    }
}

impl EventEmitter<Submit> for Composer {}
impl EntityInputHandler for Composer {
    fn text_for_range(
        &mut self,
        asked: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = range(self.text(), asked);
        *actual = Some(units(self.text(), r.start)..units(self.text(), r.end));
        Some(self.edit.text[r].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let r = self.edit.selection();
        Some(UTF16Selection {
            range: units(self.text(), r.start)..units(self.text(), r.end),
            reversed: self.edit.cursor < self.edit.anchor,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.edit
            .marked
            .as_ref()
            .map(|r| units(self.text(), r.start)..units(self.text(), r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.edit.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit.replace(r, text, false, None);
        self.reveal = true;
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit.replace(r, text, true, selected);
        self.reveal = true;
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.bounds?;
        let p =
            self.position(byte(self.text(), r.start)) + bounds.origin - point(px(0.), self.scroll);
        Some(Bounds::new(p, size(px(1.), px(self.font.line_height()))))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(units(self.text(), self.index(p)))
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = cx.entity();
        let painter = input.clone();
        div()
            .id("chat-composer")
            .debug_selector(|| "chat-composer".into())
            .role(Role::TextInput)
            .aria_label("Message Claude")
            .aria_value(SharedString::from(self.text().to_owned()))
            .w_full()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|this, _: &crate::actions::Copy, w, cx| {
                this.key(&crate::actions::edit_key("c"), w, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::Cut, w, cx| {
                this.key(&crate::actions::edit_key("x"), w, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::Paste, w, cx| {
                this.key(&crate::actions::edit_key("v"), w, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::actions::SelectAll, w, cx| {
                this.key(&crate::actions::edit_key("a"), w, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    cx.stop_propagation();
                    if !this.composing() {
                        this.edit.move_to(this.index(e.position), e.modifiers.shift);
                        this.dragging = true;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.dragging && !this.composing() {
                    this.edit.move_to(this.index(e.position), true);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let total: Pixels = this
                    .lines
                    .iter()
                    .map(|line| line.size(px(this.font.line_height())).height)
                    .sum();
                let visible = this.bounds.map_or(px(0.), |b| b.size.height);
                this.scroll = (this.scroll - e.delta.pixel_delta(px(this.font.line_height())).y)
                    .clamp(px(0.), (total - visible).max(px(0.)));
                this.reveal = false;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let input = input.read(cx);
                        let placeholder = input.text().is_empty();
                        let text: SharedString = if placeholder {
                            "Message Claude…".into()
                        } else {
                            input.text().to_owned().into()
                        };
                        let selection = input.edit.selection();
                        let mut boundaries = vec![0, text.len()];
                        if !placeholder {
                            boundaries.extend([selection.start, selection.end]);
                            if let Some(marked) = &input.edit.marked {
                                boundaries.extend([marked.start, marked.end]);
                            }
                        }
                        boundaries.sort_unstable();
                        boundaries.dedup();
                        let runs: Vec<_> = boundaries
                            .windows(2)
                            .map(|pair| TextRun {
                                len: pair[1] - pair[0],
                                font: input.font.font(),
                                color: rgb(if placeholder {
                                    input.theme.muted
                                } else {
                                    input.theme.foreground
                                })
                                .into(),
                                background_color: (!placeholder
                                    && input.focus.is_focused(window)
                                    && selection.contains(&pair[0]))
                                .then(|| rgb(input.theme.active).into()),
                                underline: input
                                    .edit
                                    .marked
                                    .as_ref()
                                    .filter(|r| r.contains(&pair[0]))
                                    .map(|_| UnderlineStyle {
                                        thickness: px(1.),
                                        ..Default::default()
                                    }),
                                strikethrough: None,
                            })
                            .collect();
                        window
                            .text_system()
                            .shape_text(
                                text,
                                px(input.font.size),
                                &runs,
                                Some(bounds.size.width.max(px(1.))),
                                None,
                            )
                            .map(|lines| lines.into_vec())
                            .unwrap_or_default()
                    },
                    move |bounds, lines, window, cx| {
                        painter.update(cx, |input, cx| {
                            let height = px(input.font.line_height());
                            input.lines = lines;
                            input.bounds = Some(bounds);
                            let caret = input.position(input.edit.cursor);
                            if input.reveal {
                                input.scroll = input
                                    .scroll
                                    .min(caret.y)
                                    .max(caret.y + height - bounds.size.height)
                                    .max(px(0.));
                                input.reveal = false;
                            }
                            window.handle_input(
                                &input.focus,
                                ViewInputHandler::new(bounds, cx.entity()),
                                cx,
                            );
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                let mut origin = bounds.origin - point(px(0.), input.scroll);
                                for line in &input.lines {
                                    let _ = line.paint(
                                        origin,
                                        height,
                                        TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    );
                                    origin.y += line.size(height).height;
                                }
                                if input.focus.is_focused(window) {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            bounds.origin + caret - point(px(0.), input.scroll),
                                            size(px(1.), height),
                                        ),
                                        rgb(input.theme.cursor),
                                    ));
                                }
                            });
                        });
                    },
                )
                .w_full()
                .h(px(self.font.line_height() * 3.)),
            )
    }
}

#[cfg(test)]
mod tests;
