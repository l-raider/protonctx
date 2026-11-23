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
    preset_from(std::env::args().skip(1))
}

/// The `--preset` mode named in `args` (the command line without the program
/// name): `Some(true)` for dark, `Some(false)` for light. The first known mode
/// wins; unknown modes are reported and skipped.
fn preset_from(mut args: impl Iterator<Item = String>) -> Option<bool> {
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

#[cfg(test)]
mod tests {
    use super::{preset_arg, preset_from};

    fn preset_of(args: &[&str]) -> Option<bool> {
        preset_from(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn preset_from_reads_both_argument_forms() {
        assert_eq!(preset_of(&["--preset", "dark"]), Some(true));
        assert_eq!(preset_of(&["--preset", "light"]), Some(false));
        assert_eq!(preset_of(&["--preset=dark"]), Some(true));
        assert_eq!(preset_of(&["--preset=light"]), Some(false));
    }

    #[test]
    fn preset_from_skips_unknown_modes_and_other_arguments() {
        assert_eq!(preset_of(&[]), None);
        assert_eq!(preset_of(&["--verbose", "dark"]), None);
        assert_eq!(preset_of(&["--preset=sepia"]), None);
        assert_eq!(
            preset_of(&["--preset=sepia", "--preset", "dark"]),
            Some(true)
        );
        // A trailing `--preset` with no value names no mode.
        assert_eq!(preset_of(&["--preset"]), None);
    }

    #[test]
    fn preset_arg_finds_no_preset_in_the_test_harness_arguments() {
        // The test binary is never started with `--preset`.
        assert_eq!(preset_arg(), None);
    }
}
