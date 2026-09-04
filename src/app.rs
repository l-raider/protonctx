//! Application wiring: load games, populate the UI model, and handle callbacks.

use std::rc::Rc;

use slint::{ModelRc, VecModel};

use crate::launcher::{self, tools};
use crate::models::Game;

// The generated Slint types (from ui/main.slint).
slint::include_modules!();

/// Human-readable about text shown in the About dialog.
const ABOUT_TEXT: &str = "protonctx 0.1.0\nLaunch executables inside a Steam game's Proton context.\n\nLicensed under the GNU GPL v3.";

pub fn run() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;

    // Load the installed games and populate the UI model.
    let games = crate::steam::discover_games();
    let model = build_game_model(&games);
    window.set_games(model.clone());

    // Built-in tools shown in the action bar.
    let tools_model: Vec<Tool> = tools::BUILTIN_TOOLS
        .iter()
        .map(|t| Tool {
            label: t.label.into(),
            arg: t.arg.into(),
        })
        .collect();
    window.set_tools(Rc::new(VecModel::from(tools_model)).into());

    window.set_about_text(ABOUT_TEXT.into());

    // Keep the `Game` structs for the selection index and launching.
    let games_rc = Rc::new(games);

    // select-game: store the selected index in the UI.
    let weak = window.as_weak();
    window.on_select_game(move |idx| {
        if let Some(win) = weak.upgrade() {
            win.set_selected_index(idx);
        }
    });

    // browse-exe: open the XDG-portal file dialog, then launch the chosen .exe.
    let weak = window.as_weak();
    let games_browse = games_rc.clone();
    window.on_browse_exe(move || {
        let Some(win) = weak.upgrade() else { return };
        let idx = win.get_selected_index();
        let Some(game) = selected_game(&games_browse, idx) else {
            return;
        };

        let dialog = rfd::FileDialog::new()
            .set_title("Select executable to run")
            .add_filter("Windows executables", &["exe", "EXE", "bat", "cmd"])
            .add_filter("All files", &["*"]);

        if let Some(path) = dialog.pick_file() {
            if let Err(e) = launcher::proton::run_in_prefix(game, &[path.to_string_lossy().as_ref()]) {
                eprintln!("protonctx: launch failed: {e}");
            }
        }
    });

    // launch-tool: launch a built-in Wine tool for the selected game.
    let weak = window.as_weak();
    let games_tool = games_rc.clone();
    window.on_launch_tool(move |arg| {
        let Some(win) = weak.upgrade() else { return };
        let idx = win.get_selected_index();
        let Some(game) = selected_game(&games_tool, idx) else {
            return;
        };

        if let Err(e) = launcher::launch_tool(game, arg.as_str()) {
            eprintln!("protonctx: launch failed: {e}");
        }
    });

    window.run()
}

/// Build the Slint model of display rows from the discovered games.
fn build_game_model(games: &[Game]) -> ModelRc<GameInfo> {
    let rows: Vec<GameInfo> = games
        .iter()
        .map(|g| GameInfo {
            name: g.name.clone().into(),
            app_id: g.app_id.to_string().into(),
            compat_tool: display_compat_tool(g),
        })
        .collect();
    Rc::new(VecModel::from(rows)).into()
}

/// Return the [`Game`] at `idx`, or `None` if out of range.
fn selected_game(games: &[Game], idx: i32) -> Option<&Game> {
    if idx < 0 {
        return None;
    }
    games.get(idx as usize)
}

/// Produce the value shown in the "Compatibility Tool" column.
///
/// When Steam has an explicit mapping (per-app `CompatToolMapping`), show that name.
/// Otherwise fall back to a short description of the resolved Proton directory, or a
/// generic "(default)" when nothing is known.
fn display_compat_tool(game: &Game) -> slint::SharedString {
    if !game.compat_tool.is_empty() {
        return game.compat_tool.clone().into();
    }

    if !game.proton_dir.is_empty() {
        if let Some(name) = std::path::Path::new(&game.proton_dir).file_name().and_then(|n| n.to_str()) {
            return format!("{name} (default)").into();
        }
    }

    "(default)".into()
}
