//! protonctx — launch executables inside a Steam game's Proton context.
//!
//! The whole application is safe Rust: the UI is a GPUI view
//! ([`views::main_ui::ProtonctxApp`]) and the Steam/Proton logic lives in the
//! pure-Rust backend modules.

#![forbid(unsafe_code)]
// Test fixtures may panic on setup failure (`unwrap` is fine in tests); the
// `unwrap_used = "deny"` workspace lint applies to production code only.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod assets;
mod config;
mod flatpak;
mod games;
mod launcher;
mod log;
mod menu;
mod models;
mod steam;
mod system_theme;
mod theme;
mod views;

use gpui_kit::*;

gpui_kit::actions!(protonctx, [RefreshGames, LogCopy, LogSelectAll, LogClear]);

fn main() {
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(|cx| {
            gpui_kit::init(cx);

            // Scrollbar policy + the static KDE theme read once at startup.
            // Do not observe `window.appearance()`: the platform appearance
            // starts as Light until the portal answers, which would flash the
            // wrong variant (P27).
            theme::init(cx);

            // Global action chords (F5 refresh, Ctrl-C/A/L log actions) are
            // defined next to the view so tests dispatch the same bindings.
            views::main_ui::bind_keys(cx);

            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(760.), px(520.)), cx)),
                    window_min_size: Some(size(px(468.), px(288.))),
                    app_id: Some("protonctx".to_string()),
                    titlebar: Some(TitlebarOptions {
                        title: Some(format!("protonctx v{}", env!("CARGO_PKG_VERSION")).into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| views::main_ui::ProtonctxApp::new(window, cx)),
            )
            .expect("failed to open window");
        });
}
