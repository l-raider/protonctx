//! GPUI views. The four legacy Qt translation units keep their identities
//! (`about_ui`, `logs_ui`, `main_ui`, `settings_ui`); [`dialog_ui`] is the shared
//! native-dialog template and chrome and [`widgets`] the shared button recipes,
//! neither a translation unit.

pub mod about_ui;
pub mod dialog_ui;
pub mod logs_ui;
pub mod main_ui;
pub mod settings_ui;
pub mod widgets;
