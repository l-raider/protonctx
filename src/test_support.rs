//! Test-only helpers shared across the crate's unit tests.

use crate::models::Game;

/// A fresh temp directory named for `tag` and this process.
///
/// Each test uses a unique `tag`, so parallel tests never share a directory.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("protonctx_test_{tag}_{}", std::process::id()))
}

/// A [`Game`] fixture with every field populated.
pub fn game(
    name: &str,
    app_id: u32,
    compat_tool: &str,
    library_path: &str,
    proton_dir: &str,
) -> Game {
    Game {
        name: name.to_string(),
        app_id,
        compat_tool: compat_tool.to_string(),
        library_path: library_path.to_string(),
        proton_dir: proton_dir.to_string(),
    }
}

/// The common two-argument fixture: a known tool and library layout.
pub fn test_game(name: &str, app_id: u32) -> Game {
    game(
        name,
        app_id,
        "proton_experimental",
        "/lib",
        "/lib/steamapps/common/Proton - Experimental",
    )
}

/// A fresh [`State`](crate::app::State) for a main window that is never
/// opened; nothing touches a real window or the Steam library.
pub fn state() -> crate::app::State {
    crate::app::State::with_main_window(iced::window::Id::unique())
}

/// A [`state`] showing `games` as rows, in the given order.
pub fn state_with(games: &[Game]) -> crate::app::State {
    let mut state = state();
    state.set_rows(games.iter().map(crate::games::GameRow::from_game).collect());
    // `update` mirrors the rows into the table columns; this fixture bypasses
    // it, so mirror them here.
    state.sync_table();
    state
}
