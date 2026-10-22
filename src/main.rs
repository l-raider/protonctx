//! protonctx — launch executables inside a Steam game's Proton context.
//!
//! All hand-written code is safe Rust: the UI is Slint ([`ui`], driven by
//! [`app`]) on X11 with the software renderer, and the Steam/Proton logic
//! lives in the pure-Rust backend modules.

// `deny`, not `forbid`: the code Slint generates from `ui/*.slint` (static
// item-tree vtables) carries its own `#[allow(unsafe_code)]`, which `forbid`
// would reject. Hand-written code here still cannot use `unsafe`.
#![deny(unsafe_code)]
// Test fixtures may panic on setup failure (`unwrap` is fine in tests); the
// `unwrap_used = "deny"` lint applies to production code only.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod app;
mod background;
mod config;
mod flatpak;
mod games;
mod launcher;
mod log;
mod log_buffer;
mod models;
mod state;
mod steam;
mod system_theme;
#[cfg(test)]
mod test_support;
mod theme;
mod ui;
#[cfg(test)]
mod visual_tests;
mod x11_dialog;
mod xdg;

use slint::ComponentHandle as _;

/// `--preset light|dark` forces the bundled Breeze preset instead of the live
/// KDE scheme (a development switch for spot checks).
fn preset_arg() -> Option<bool> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = match arg.strip_prefix("--preset=") {
            Some(value) => Some(value.to_string()),
            None if arg == "--preset" => args.next(),
            None => None,
        };
        match value.as_deref() {
            Some("light") => return Some(false),
            Some("dark") => return Some(true),
            Some(other) => eprintln!("protonctx: unknown preset mode {other:?} (light|dark)"),
            None => {}
        }
    }
    None
}

fn main() -> Result<(), slint::PlatformError> {
    // The static desktop theme, read once before the first window opens.
    let tokens = theme::build(&system_theme::load(preset_arg()));

    ui::select_backend()?;
    let main = ui::MainWindow::new()?;
    ui::apply_theme(&main, &tokens);
    let _app = app::start(&main, tokens);
    main.show()?;
    slint::run_event_loop_until_quit()
}
