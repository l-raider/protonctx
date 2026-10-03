use gpui_kit::component::{Theme, scroll::ScrollbarMode};
use gpui_kit::*;

/// Applies the static desktop theme once at startup and makes the scrollbar
/// policy explicit.
pub fn init(cx: &mut App) {
    Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);
    crate::system_theme::init(cx);
}
