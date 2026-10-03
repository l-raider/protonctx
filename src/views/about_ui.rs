//! The About dialog, opened from the toolbar menu.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, button::Button, h_flex, v_flex};
use gpui_kit::*;

use crate::views::dialog_ui;

/// Open the About dialog. The version comes from `CARGO_PKG_VERSION`
/// (Cargo.toml is the single source of truth).
pub fn open_about_dialog(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _window, cx| {
        dialog
            .title("About protonctx")
            .w(px(360.))
            .child(
                v_flex()
                    .w_full()
                    .items_center()
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
                        // `w_full + text_center` bounds the paragraph so a long
                        // description wraps instead of widening the dialog.
                        dialog_ui::dialog_text(
                            "Launch executables inside a Steam game's Proton context.",
                        )
                        .w_full()
                        .text_center(),
                    )
                    .child(
                        dialog_ui::dialog_text("Licensed under the GNU GPL v3.")
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
            .footer(
                h_flex().w_full().justify_end().child(
                    Button::new("about-ok")
                        .label("Ok")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
