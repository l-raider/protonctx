//! GPUI views. The four legacy Qt translation units keep their identities
//! (`about_ui`, `logs_ui`, `main_ui`, `settings_ui`); [`dialog_ui`] is shared
//! dialog chrome, not a translation unit.

pub mod about_ui;
pub mod dialog_ui;
pub mod logs_ui;
pub mod main_ui;
pub mod settings_ui;
