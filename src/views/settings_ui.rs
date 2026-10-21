//! The Settings dialog body, opened from the toolbar menu.
//!
//! The checkbox is bound to `State::remember_last_dir` and persists
//! immediately on change (the legacy Qt dialog did the same), so the value
//! survives even if the app is killed before exit.

use iced::widget::checkbox;
use iced::{Background, Border, Element, Theme};

use crate::app::Message;
use crate::theme::tokens;
use crate::views::widgets::faded;

pub fn body<'a>(remember_last_dir: bool) -> Element<'a, Message> {
    let t = tokens();
    checkbox(remember_last_dir)
        .label("Remember last used directory")
        .on_toggle(Message::RememberLastDir)
        // gpui's checkbox: a 1 rem box, 0.5 rem from its label.
        .size(t.font_size.round())
        .spacing(t.rem(2.0))
        .text_size(t.font_size)
        .font(t.font)
        .style(|_theme: &Theme, status| checkbox_style(status))
        .into()
}

/// A solid `primary` box with a `primary_foreground` check when checked; the
/// outline-button border and fill when not. Hover outlines it in `primary`.
fn checkbox_style(status: checkbox::Status) -> checkbox::Style {
    let t = tokens();
    let (checked, hovered, disabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let (fill, border) = if checked {
        (t.primary, t.primary)
    } else if hovered {
        (t.input_background, t.primary)
    } else {
        (t.input_background, t.input_border)
    };
    let fade = |color| {
        if disabled {
            faded(color, t.disabled_opacity)
        } else {
            color
        }
    };
    checkbox::Style {
        background: Background::Color(fade(fill)),
        icon_color: fade(t.primary_foreground),
        border: Border {
            color: fade(border),
            width: 1.0,
            radius: (t.radius - 1.0).max(0.0).into(),
        },
        text_color: Some(fade(t.foreground)),
    }
}
