//! Settings > Appearance > Window backdrop: how solid the main window's
//! background is, whether the desktop behind it blurs, and a picture behind
//! it. Settings and dialogs stay opaque whatever is chosen.
use super::*;
use crate::config::backdrop::{Edit, IMAGE_OPACITY};

/// The opacities offered, in percent and most solid first. A config file
/// may set any other, which then shows with none of these selected.
const OPACITIES: [u8; 8] = [100, 95, 90, 85, 80, 70, 60, 50];

impl SettingsWindow {
    pub(in crate::settings_window) fn render_backdrop_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Div {
        let backdrop = &self.config.backdrop;
        let percent = (backdrop.opacity * 100.).round();
        let mut levels = div().flex().flex_wrap().gap(px(8.));
        for level in OPACITIES {
            let id = SharedString::from(format!("settings-backdrop-opacity-{level}"));
            let selected = f32::from(level) == percent;
            levels = levels.child(
                self.control_choice(id.clone(), format!("{level}%"), selected, true)
                    .debug_selector(move || id.to_string())
                    .when(!selected, |choice| {
                        choice.on_click(cx.listener(move |this, _, _, cx| {
                            let edit = Edit::Opacity(f32::from(level) / 100.);
                            this.save_native(move || Config::save_backdrop(edit), cx);
                        }))
                    }),
            );
        }
        let picture = backdrop.image.as_deref().map(|image| {
            image
                .file_name()
                .unwrap_or(image.as_os_str())
                .to_string_lossy()
                .into_owned()
        });
        let has_picture = picture.is_some();
        let mut actions = div().flex().flex_wrap().gap(px(8.)).child(
            self.control_choice(
                "settings-backdrop-choose",
                "Choose picture\u{2026}",
                false,
                true,
            )
            .debug_selector(|| "settings-backdrop-choose".into())
            .on_click(cx.listener(|this, _, _, cx| this.choose_backdrop_picture(cx))),
        );
        if has_picture {
            actions = actions.child(
                self.control_choice("settings-backdrop-remove", "Remove", false, true)
                    .debug_selector(|| "settings-backdrop-remove".into())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.save_native(|| Config::save_backdrop(Edit::Image(None)), cx);
                    })),
            );
        }
        // A picture covers the desktop, so there is nothing behind to blur.
        let blurs = backdrop.translucent() && !has_picture;
        let blur = backdrop.blur;
        self.control_card("Window backdrop")
            .debug_selector(|| "settings-backdrop".into())
            .child(self.control_note(
                "How solid the main window's background is. Terminal colors a program chooses, popups, and dialogs stay opaque.",
            ))
            .child(levels)
            .child(
                self.control_switch("settings-backdrop-blur", "Blur behind", blur, blurs)
                    .when(blurs, |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| {
                            this.save_native(move || Config::save_backdrop(Edit::Blur(!blur)), cx);
                        }))
                    }),
            )
            .child(self.control_row(
                "Picture",
                picture.unwrap_or_else(|| "None".into()),
            ))
            .child(actions)
            .child(self.control_note(
                "A picture fills the window behind the translucent background; the lower the opacity, the more of it shows.",
            ))
    }

    fn choose_backdrop_picture(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        // An opaque background would hide the picture just chosen.
        let opaque = !self.config.backdrop.translucent();
        cx.spawn(async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Could not open the picture chooser");
                    None
                }
                Ok(Ok(None)) | Err(_) => None,
            };
            let Some(path) = path else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.save_native(
                    move || {
                        // Refused here, the error shows in Settings rather
                        // than only in the log once the window tries it.
                        crate::backdrop::decode(&path)?;
                        Config::save_backdrop(Edit::Image(Some(path)))?;
                        if opaque {
                            Config::save_backdrop(Edit::Opacity(IMAGE_OPACITY))?;
                        }
                        Ok(())
                    },
                    cx,
                );
            });
        })
        .detach();
    }
}
