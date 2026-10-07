//! Shared Breeze-style buttons. Theme colors are copied to locals so the
//! hover/active/focus closures can move them (the call sites do the same).

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Breeze outline button used by the action row and every dialog action.
/// Disabled state dims the outline exactly like the legacy action row.
pub fn outline_button(id: &'static str, enabled: bool, cx: &App) -> BaseButton {
    let theme = cx.theme();
    let (input, primary, ring, radius) = (theme.input, theme.primary, theme.ring, theme.radius);
    let input_bg = theme.input_background();
    let pressed_bg = theme.tokens.accent.background;
    let fg = theme.button_foreground;
    let disabled_fg = theme.muted_foreground.opacity(0.5);

    BaseButton::new(id)
        .h(px(32.))
        .min_w(px(80.))
        .px(px(14.))
        .rounded(radius)
        .border_1()
        .text_base()
        .when(enabled, |this| {
            this.border_color(input)
                .bg(input_bg)
                .text_color(fg)
                .hover(move |style| style.border_color(primary))
                .active(move |style| style.bg(pressed_bg).border_color(primary))
                .focus_visible(move |style| style.border_color(ring))
        })
        .when(!enabled, |this| {
            this.border_color(input.opacity(0.5))
                .bg(input_bg.opacity(0.5))
                .text_color(disabled_fg)
        })
        .disabled(!enabled)
}

/// Toolbar icon button: the same transparent custom variant for the main menu
/// and the refresh action, so the toolbar cannot drift from itself.
pub fn toolbar_icon_button(id: &'static str, cx: &App) -> Button {
    Button::new(id).custom(
        ButtonCustomVariant::new(cx)
            .color(cx.theme().transparent)
            .foreground(cx.theme().primary_foreground)
            .hover(cx.theme().primary)
            .active(cx.theme().primary),
    )
}
