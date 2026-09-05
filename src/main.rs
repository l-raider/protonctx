//! protonctx — launch executables inside a Steam game's Proton context.

// Test fixtures may panic on setup failure (`unwrap` is fine in tests); the
// `unwrap_used = "deny"` workspace lint applies to production code only.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod app_backend;
mod config;
mod launcher;
mod models;
mod steam;

// FFI to the hand-written C++ Qt Widgets UI
unsafe extern "C" {
    fn qt_app_init();
    fn qt_show_main_window();
    fn qt_app_exec() -> i32;
    fn qt_load_games();
}

fn main() -> std::process::ExitCode {
    // Create QApplication (Qt Widgets) so native dialogs work.
    unsafe { qt_app_init() };

    unsafe { qt_show_main_window() };

    // Populate the games table after the window exists (model attached in C++).
    unsafe { qt_load_games() };

    std::process::ExitCode::from(unsafe { qt_app_exec() } as u8)
}
