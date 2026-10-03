//! The Settings dialog, opened from the toolbar menu.
//!
//! The checkbox is bound to the root's `remember_last_directory` and persists
//! immediately on change (the legacy Qt dialog did the same), so the value
//! survives even if the app is killed before exit.

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::*;

use crate::views::dialog_ui::{self, DialogAction, DialogActionKind, DialogContent, DialogSize};
use crate::views::main_ui::ProtonctxApp;

/// Body of the native Settings dialog; the template provides the window chrome.
///
/// `checked` is a snapshot because the first frame renders while the owning
/// entity is being updated; the checkbox handler pushes the change back.
struct SettingsContent {
    app: WeakEntity<ProtonctxApp>,
    /// The main window: the change handler must run with *its* window, because
    /// `apply_remember_last_directory` appends to the log pane and schedules
    /// the frame on the owning window (not on this dialog).
    main: AnyWindowHandle,
    checked: bool,
}

impl DialogContent for SettingsContent {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn title(&self) -> SharedString {
        "Settings".into()
    }

    fn size(&self) -> DialogSize {
        DialogSize::Compact
    }

    fn body(&mut self, _window: &mut Window, _cx: &mut App) -> AnyElement {
        let checked = self.checked;
        let app = self.app.clone();
        let main = self.main;

        Checkbox::new("remember-last-directory")
            .label("Remember last used directory")
            .checked(checked)
            .on_change(move |value, _window, cx| {
                let _ = dialog_ui::with_window_and_entity(main, &app, cx, |app, window, cx| {
                    app.apply_remember_last_directory(*value, window, cx);
                });
                let _ = dialog_ui::update_dialog_content::<SettingsContent>(
                    "settings",
                    cx,
                    |content| content.checked = *value,
                );
            })
            .into_any_element()
    }

    fn actions(&self, _cx: &App) -> Vec<DialogAction> {
        vec![
            DialogAction::close("settings-cancel", "Cancel"),
            DialogAction::new("settings-ok", "OK", DialogActionKind::Primary),
        ]
    }
}

/// Open the Settings dialog against the root app entity. `checked` is the
/// current value, read by the caller because the dialog's first frame renders
/// while the caller still holds the owning entity.
pub fn open_settings_dialog(
    checked: bool,
    app: WeakEntity<ProtonctxApp>,
    window: &mut Window,
    cx: &mut App,
) {
    let content = SettingsContent {
        app,
        main: window.window_handle(),
        checked,
    };
    let _ = dialog_ui::open_dialog(window, cx, content);
}
