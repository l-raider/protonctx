//! The About dialog, opened from the toolbar menu.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, button::Button, h_flex, v_flex};
use gpui_kit::*;

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
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(concat!("protonctx v", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_center()
                            .child("Launch executables inside a Steam game's Proton context."),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Licensed under the GNU GPL v3."),
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
