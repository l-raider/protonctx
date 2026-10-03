//! Shared dialog chrome: message text and the icon + message body.
//!
//! Not a legacy Qt translation unit: `about_ui` and `main_ui` render this
//! chrome so the wrapping and typography of their dialogs cannot drift apart.
//! The wrapping layout mirrors `AlertDialog`'s body: the text column is
//! `flex_1().min_w_0()`, without which the flex item's auto minimum size keeps
//! the text at its intrinsic (unwrapped) width and the dialog's
//! `overflow_hidden` body clips it.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::*;

/// Width of the message dialogs (Launch Error, Delete Shader Cache).
pub const MESSAGE_DIALOG_WIDTH: Pixels = px(420.);

/// Dialog body copy at Qt's 10 pt scale: base text size with a 1.25 line
/// height, matching the rem the root plugin installs on KDE (10 pt @ 96 DPI).
pub fn dialog_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_base()
        .line_height(relative(1.25))
        .child(text.into())
}

/// The shared alert body: warning icon plus message text that wraps inside the
/// dialog instead of relying on the clip.
pub fn dialog_alert_body(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .child(
            Icon::new(IconName::CircleAlert)
                .flex_shrink_0()
                .text_color(cx.theme().danger),
        )
        .child(v_flex().flex_1().min_w_0().child(dialog_text(text)))
}
