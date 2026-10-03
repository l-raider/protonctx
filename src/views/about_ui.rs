//! The About dialog, opened from the toolbar menu.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, v_flex};
use gpui_kit::*;

use crate::views::dialog_ui::{self, DialogAction, DialogActionKind, DialogContent, DialogSize};

/// Body of the About dialog; the template provides the window chrome.
struct AboutContent;

impl DialogContent for AboutContent {
    fn id(&self) -> &'static str {
        "about"
    }

    fn title(&self) -> SharedString {
        "About protonctx".into()
    }

    fn size(&self) -> DialogSize {
        DialogSize::Form
    }

    fn body(&mut self, _window: &mut Window, cx: &mut App) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::Gamepad2)
                    .size(px(64.))
                    .text_color(cx.theme().primary),
            )
            .child(
                dialog_ui::dialog_text(concat!("protonctx v", env!("CARGO_PKG_VERSION")))
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                dialog_ui::dialog_text("Launch executables inside a Steam game's Proton context.")
                    .w_full()
                    .text_center(),
            )
            .child(
                dialog_ui::dialog_text("Licensed under the GNU GPL v3.")
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element()
    }

    fn actions(&self, _cx: &App) -> Vec<DialogAction> {
        vec![DialogAction::new(
            "about-ok",
            "Ok",
            DialogActionKind::Primary,
        )]
    }
}

/// Open the About dialog as a native window. The version comes from
/// `CARGO_PKG_VERSION` (Cargo.toml is the single source of truth).
pub fn open_about_dialog(window: &mut Window, cx: &mut App) {
    let _ = dialog_ui::open_dialog(window, cx, AboutContent);
}
