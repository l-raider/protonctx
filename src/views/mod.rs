//! iced views. Each is a pure function of [`crate::app::State`]; the
//! legacy Qt translation units keep their identities (`about_ui`, `logs_ui`,
//! `main_ui`, `settings_ui`), [`dialog_ui`] is the shared dialog-window
//! template, [`table`] the games table and [`widgets`] the shared Breeze
//! recipes.

pub mod about_ui;
pub mod dialog_ui;
pub mod logs_ui;
pub mod main_ui;
pub mod settings_ui;
pub mod table;
pub mod widgets;
