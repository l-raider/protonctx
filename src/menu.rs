//! Popup-menu items rendered at the theme's base font size.
//!
//! gpui-component hardcodes `.text_sm()` (0.875 rem) on every drawn popup-menu
//! item (`menu/popup_menu.rs`), which renders one step smaller than the base
//! font KDE uses for menus and smaller than the buttons and table headers next
//! to them. The library exposes no menu text-size token, so the app builds its
//! labels as elements and pins them to `theme.font_size`. Standard item
//! behaviour (icon, disabled, checked, `on_click`/action) is unchanged.
//!
//! [`app_item`] adds the app-entity click wiring so the toolbar and the games
//! context menu cannot drift apart.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, menu::PopupMenuItem};
use gpui_kit::*;

/// A popup-menu item whose label uses the theme's base font size.
pub fn item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    PopupMenuItem::element(move |_window, cx| {
        div().text_size(cx.theme().font_size).child(label.clone())
    })
}

/// A popup-menu item that runs `handler` on the app `entity` when clicked.
///
/// `Window::listener_for` returns a repeatable listener, so the handler is an
/// `Fn` (not `FnOnce`). An optional leading icon matches the library's
/// `PopupMenuItem::icon`.
pub fn app_item<T: 'static>(
    window: &Window,
    entity: &Entity<T>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    handler: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> PopupMenuItem {
    let mut entry = item(label).on_click(window.listener_for(
        entity,
        move |app, _: &ClickEvent, window, cx| {
            handler(app, window, cx);
        },
    ));
    if let Some(icon) = icon {
        entry = entry.icon(icon);
    }
    entry
}
