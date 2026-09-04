//! Application wiring: load games, populate the UI model, and handle callbacks.

use std::rc::Rc;

use slint::{Model, ModelExt, ModelRc, StandardListViewItem, VecModel};

use crate::launcher::{self, tools};
use crate::models::Game;

// The generated Slint types (from ui/main.slint).
slint::include_modules!();

/// Human-readable about text shown in the About dialog.
const ABOUT_TEXT: &str = "protonctx 0.1.0\nLaunch executables inside a Steam game's Proton context.\n\nLicensed under the GNU GPL v3.";

pub fn run() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;

    // Load the installed games and keep the `Game` list for launching. The table shows
    // the same order; selection resolves back into this list via the sorted row model.
    let games = crate::steam::discover_games();
    let games_rc = Rc::new(games);

    // Build the display rows (game name, app id, compat tool) and give them to the
    // table adapter. The adapter's `sort` callback (wired below) re-sorts these rows
    // in Rust, so the table's current-row always indexes into a stable sorted order.
    let row_model = build_game_model(&games_rc);
    window.global::<TableAdapter>().set_rows(row_model);

    // Built-in tools shown in the action bar.
    let tools_model: Vec<Tool> = tools::BUILTIN_TOOLS
        .iter()
        .map(|t| Tool {
            label: t.label.into(),
            arg: t.arg.into(),
        })
        .collect();
    window.set_tools(Rc::new(VecModel::from(tools_model)).into());

    // sort: given the (previously sorted/filtered) row model, return a new model sorted
    // by the requested column. Runs on every sort request from the table headers.
    window.global::<TableAdapter>().on_sort(|rows, column, ascending| {
        sort_rows(rows, column, ascending)
    });

    // browse-exe: open the XDG-portal file dialog, then launch the chosen .exe for the
    // selected game (selection resolves into the sorted row model, so it always matches
    // the visible row even after sorting).
    let weak = window.as_weak();
    let games_browse = games_rc.clone();
    window.on_browse_exe(move || {
        let Some(win) = weak.upgrade() else { return };
        let Some(game) = selected_game(&games_browse, &win) else {
            return;
        };

        let dialog = rfd::FileDialog::new()
            .set_title("Select executable to run")
            .add_filter("Windows executables", &["exe", "EXE", "bat", "cmd"])
            .add_filter("All files", &["*"]);

        if let Some(path) = dialog.pick_file() {
            if let Err(e) = launcher::proton::run_in_prefix(game, &[path.to_string_lossy().as_ref()]) {
                show_launch_error(&win, &e);
            }
        }
    });

    // launch-tool: launch a built-in Wine tool for the selected game.
    let weak = window.as_weak();
    let games_tool = games_rc.clone();
    window.on_launch_tool(move |arg| {
        let Some(win) = weak.upgrade() else { return };
        let Some(game) = selected_game(&games_tool, &win) else {
            return;
        };

        if let Err(e) = launcher::launch_tool(game, arg.as_str()) {
            show_launch_error(&win, &e);
        }
    });

    // show-settings: open the (placeholder) settings dialog, centered on the main window.
    let weak = window.as_weak();
    window.on_show_settings(move || {
        let Some(win) = weak.upgrade() else { return };
        if let Ok(dialog) = SettingsDialog::new() {
            center_on_parent(&dialog, win.window(), (360.0, 180.0));
            let _ = dialog.show();
        }
    });

    // show-about: open the about dialog with version info, centered on the main window.
    let weak = window.as_weak();
    window.on_show_about(move || {
        let Some(win) = weak.upgrade() else { return };
        if let Ok(dialog) = AboutDialog::new() {
            dialog.set_about_text(ABOUT_TEXT.into());
            center_on_parent(&dialog, win.window(), (380.0, 200.0));
            let _ = dialog.show();
        }
    });

    window.run()
}

/// Build the Slint table model (a model of rows, each a model of cells) from the games.
/// Cell order: name, app id, compat tool — matching the table's columns.
fn build_game_model(games: &[Game]) -> ModelRc<ModelRc<StandardListViewItem>> {
    let row_vec: Vec<ModelRc<StandardListViewItem>> = games
        .iter()
        .map(|g| {
            let cells: Vec<StandardListViewItem> = vec![
                slint::format!("{}", g.name).into(),
                slint::format!("{}", g.app_id).into(),
                slint::format!("{}", display_compat_tool(g)).into(),
            ];
            Rc::new(VecModel::from(cells)).into()
        })
        .collect();
    Rc::new(VecModel::from(row_vec)).into()
}

/// Sort the table rows by the given column (0 = name, 1 = app id, 2 = compat tool),
/// ascending or descending. When `column < 0`, returns the rows unchanged.
fn sort_rows(
    rows: ModelRc<ModelRc<StandardListViewItem>>,
    column: i32,
    ascending: bool,
) -> ModelRc<ModelRc<StandardListViewItem>> {
    if column < 0 {
        return rows;
    }

    let col = column as usize;
    Rc::new(rows.sort_by(move |a, b| {
        let cell_a = a.row_data(col).map(|c| c.text.clone()).unwrap_or_default();
        let cell_b = b.row_data(col).map(|c| c.text.clone()).unwrap_or_default();
        if ascending {
            cell_a.cmp(&cell_b)
        } else {
            cell_b.cmp(&cell_a)
        }
    }))
    .into()
}

/// Return the [`Game`] currently selected in the table. The game list and the table's
/// row model share the same order, so the table's `selected-index` (current-row) maps
/// directly into the list.
fn selected_game<'a>(games: &'a [Game], window: &MainWindow) -> Option<&'a Game> {
    let idx = window.get_selected_index();
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

/// Center `dialog` over `parent`, given the dialog's logical size in pixels.
///
/// Positions use physical screen coordinates. `set_position` is a no-op on some
/// Wayland compositors that forbid client-side window placement, which is acceptable.
fn center_on_parent<T: slint::ComponentHandle>(
    dialog: &T,
    parent: &slint::Window,
    logical_size: (f32, f32),
) {
    let parent_pos = parent.position();
    let parent_size = parent.size();
    let scale = dialog.window().scale_factor().max(1.0);

    let dialog_w = (logical_size.0 * scale) as i32;
    let dialog_h = (logical_size.1 * scale) as i32;

    let x = parent_pos.x + (parent_size.width as i32 - dialog_w) / 2;
    let y = parent_pos.y + (parent_size.height as i32 - dialog_h) / 2;

    dialog
        .window()
        .set_position(slint::PhysicalPosition::new(x, y));
}

/// Show a launch failure to the user in a proper error dialog, centered on the main window. Also keeps the message on stderr so
/// it survives even if the dialog cannot be shown.
fn show_launch_error(parent: &MainWindow, error: &launcher::LaunchError) {
    eprintln!("protonctx: launch failed: {error}");

    let Some(dialog) = LaunchErrorDialog::new().ok() else {
        return;
    };
    dialog.set_error_message(format!("Failed to launch: {error}").into());
    center_on_parent(&dialog, parent.window(), (420.0, 160.0));
    let _ = dialog.show();
}
