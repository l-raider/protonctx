//! The process-log pane: a framed, muted, read-only monospace textarea.

use gpui_kit::component::{
    ActiveTheme as _,
    input::{Textarea, TextareaState},
};
use gpui_kit::*;

/// Render the log pane. The caller owns the state and the shadow text.
pub fn render_log_pane(log: &Entity<TextareaState>, cx: &App) -> impl IntoElement {
    div()
        .size_full()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .p_1()
        .child(
            Textarea::new(log)
                .readonly(true)
                .appearance(false)
                .bordered(false)
                // The log uses the read-only Textarea (the QML TextEdit mapping), not the
                // Markdown-only TextView. Both styles override the input's `text_sm` default.
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(cx.theme().font_size)
                .size_full(),
        )
}
