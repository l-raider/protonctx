//! protonctx — launch executables inside a Steam game's Proton context.
//!
//! Phase 1 stub: the theme layer is ported and an empty `iced::daemon` opens
//! the 760×520 main window; the views follow in the next phase.

#![forbid(unsafe_code)]
// Test fixtures may panic on setup failure (`unwrap` is fine in tests); the
// `unwrap_used = "deny"` workspace lint applies to production code only.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(dead_code)]

mod config;
mod flatpak;
mod launcher;
mod log;
mod models;
mod steam;
mod system_theme;
#[cfg(test)]
mod test_support;
mod theme;
mod xdg;

use iced::widget::space;
use iced::{Element, Size, Task, window};

struct Stub;

fn boot() -> (Stub, Task<()>) {
    let (_, open) = window::open(window::Settings {
        size: Size::new(760.0, 520.0),
        min_size: Some(Size::new(468.0, 288.0)),
        position: window::Position::Centered,
        ..window::Settings::default()
    });
    (Stub, open.discard())
}

fn update(_: &mut Stub, _: ()) -> Task<()> {
    Task::none()
}

fn view(_: &Stub, _: window::Id) -> Element<'_, ()> {
    space().into()
}

fn main() -> iced::Result {
    let loaded = system_theme::load(None);
    let (iced_theme, tokens) = theme::build(&loaded);
    theme::install(tokens);

    iced::daemon(boot, update, view)
        .title(|_: &Stub, _| format!("protonctx v{}", env!("CARGO_PKG_VERSION")))
        .theme(move |_: &Stub, _| iced_theme.clone())
        .run()
}
