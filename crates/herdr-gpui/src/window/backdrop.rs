//! The main window's side of `[backdrop]`: its platform appearance, its
//! picture, and the colors its terminals and chrome paint with. The rules
//! are in `crate::backdrop`.
use super::HerdrWindow;
use crate::{
    backdrop::{self, Fill, Picture},
    terminal_painter::Ground,
};
use gpui::{prelude::*, *};

/// What one window has done about its backdrop so far.
#[derive(Default)]
pub(crate) struct State {
    picture: Picture,
    /// The running decode; replacing it cancels the one before.
    load: Option<Task<()>>,
    /// Set only when it changes: the platform call is not free.
    appearance: WindowBackgroundAppearance,
}

impl HerdrWindow {
    pub(crate) fn fill(&self) -> Fill {
        Fill::new(&self.theme, &self.config.backdrop)
    }

    pub(crate) fn ground(&self) -> Ground {
        match self.fill().translucent() {
            true => Ground::Backdrop,
            false => Ground::Opaque,
        }
    }

    /// Brings the platform appearance and the picture in line with the
    /// config. Called while rendering, where the window is at hand; each
    /// changes only when the config does, and the decode runs off this thread.
    pub(super) fn sync_backdrop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = &self.config.backdrop;
        // An opaque ground hides a picture, so none is decoded for it.
        let path = config.translucent().then(|| config.image_path()).flatten();
        let state = &mut self.backdrop;
        if let Some(path) = state.picture.want(path) {
            let decode = cx.background_executor().spawn({
                let path = path.clone();
                async move { backdrop::decode(&path) }
            });
            state.load = Some(cx.spawn(async move |this, cx| {
                let result = decode.await;
                let _ = this.update(cx, |this, cx| {
                    this.backdrop.picture.finish(path, result);
                    this.backdrop.load = None;
                    cx.notify();
                });
            }));
        }
        for texture in state.picture.take_released() {
            let _ = window.drop_image(texture);
        }
        let appearance = backdrop::appearance(config, state.picture.covers());
        if appearance != state.appearance {
            window.set_background_appearance(appearance);
            state.appearance = appearance;
        }
    }

    /// Paints the window's ground on `root`: the theme's background, solid
    /// or translucent, over the picture when there is one.
    pub(super) fn with_backdrop(&self, root: Div) -> Div {
        let fill = self.fill();
        if !fill.translucent() {
            return root.bg(rgb(self.theme.background));
        }
        let Some(texture) = self.backdrop.picture.texture() else {
            return root.bg(fill.ground());
        };
        // Absolute, so neither takes part in the column's layout, and first,
        // so everything else paints over them.
        root.child(
            img(texture)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .object_fit(ObjectFit::Cover),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(fill.ground()),
        )
    }
}
