//! protonctx — launch executables inside a Steam game's Proton context.
//!
//! The whole application is safe Rust: the UI is an `iced` daemon
//! ([`app::State`]) rendered with tiny-skia, and the Steam/Proton logic lives
//! in the pure-Rust backend modules.

#![forbid(unsafe_code)]
// Test fixtures may panic on setup failure (`unwrap` is fine in tests); the
// `unwrap_used = "deny"` workspace lint applies to production code only.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod app;
mod assets;
mod background;
mod config;
mod flatpak;
mod games;
mod launcher;
mod log;
mod menu;
mod models;
mod steam;
mod system_theme;
#[cfg(test)]
mod test_support;
mod theme;
mod views;
#[cfg(test)]
mod visual_tests;
mod x11_dialog;
mod xdg;

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

fn main() -> iced::Result {
    // The static desktop theme, read once before the first window opens. The
    // platform appearance is never observed afterwards (no live switching).
    let loaded = system_theme::load(preset_arg());
    let (iced_theme, tokens) = theme::build(&loaded);
    let (font, font_size) = (tokens.font, tokens.font_size);
    theme::install(tokens);

    iced::daemon(
        move || app::State::new(iced_theme.clone()),
        app::State::update,
        app::State::view,
    )
    .title(app::State::title)
    .theme(app::State::theme)
    .subscription(app::State::subscription)
    .default_font(font)
    .settings(iced::Settings {
        default_text_size: font_size.into(),
        ..iced::Settings::default()
    })
    .run()
}
