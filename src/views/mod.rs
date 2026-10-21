//! iced views. Each is a pure function of [`crate::app::State`]; the
//! legacy Qt translation units keep their identities (`logs_ui`, `main_ui`),
//! [`table`] is the games table and [`widgets`] the shared Breeze recipes.

pub mod logs_ui;
pub mod main_ui;
pub mod table;
pub mod widgets;
