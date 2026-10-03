//! Popup-menu items rendered at the theme's base font size.
//!
//! gpui-component hardcodes `.text_sm()` (0.875 rem) on every drawn popup-menu
//! item (`menu/popup_menu.rs`), which renders one step smaller than the base
//! font KDE uses for menus and smaller than the buttons and table headers next
//! to them. The library exposes no menu text-size token, so the app builds its
//! labels as elements and pins them to `theme.font_size`. Standard item
//! behaviour (icon, disabled, checked, `on_click`/action) is unchanged.

use gpui_kit::component::{ActiveTheme as _, menu::PopupMenuItem};
use gpui_kit::*;

/// A popup-menu item whose label uses the theme's base font size.
pub fn item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    PopupMenuItem::element(move |_window, cx| {
        div().text_size(cx.theme().font_size).child(label.clone())
    })
}
