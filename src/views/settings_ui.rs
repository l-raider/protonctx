//! The Settings dialog, opened from the toolbar menu.
//!
//! The checkbox is bound to the root's `remember_last_directory` and persists
//! immediately on change (the legacy Qt dialog did the same), so the value
//! survives even if the app is killed before exit.

use gpui_kit::component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
};
use gpui_kit::*;

use crate::views::main_ui::ProtonctxApp;

/// Open the Settings dialog against the root app entity.
pub fn open_settings_dialog(app: WeakEntity<ProtonctxApp>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let checked = app
            .read_with(cx, |app, _| app.remember_last_directory)
            .unwrap_or(true);
        let Some(entity) = app.upgrade() else {
            return dialog;
        };
        // `listener_for` (not `update_in`): the checkbox handler runs while the
        // window dispatches its own events (P3).
        let on_change = window.listener_for(&entity, |app, value: &bool, window, cx| {
            app.apply_remember_last_directory(*value, window, cx);
        });

        dialog
            .title("Settings")
            .w(px(360.))
            .child(
                Checkbox::new("remember-last-directory")
                    .label("Remember last used directory")
                    .checked(checked)
                    .on_change(on_change),
            )
            // A plain Dialog renders no buttons without an explicit footer (P17).
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("settings-cancel")
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("settings-ok")
                            .primary()
                            .label("OK")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}
