//! The games table delegate and its pure helpers.
//!
//! Row conversion, display formatting, sorting, and identity lookup are pure
//! functions unit-tested beside this module; [`GamesDelegate`] keeps the
//! prototype's behaviour (sort state persists across refreshes, selection is
//! restored by App ID across sorts, the context menu drives real actions).

use gpui_kit::component::{
    ActiveTheme as _,
    menu::{PopupMenu, PopupMenuItem},
    table::{Column, ColumnSort, TableDelegate, TableState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::models::Game;
use crate::views::main_ui::ProtonctxApp;

pub const COLUMN_KEYS: [&str; 3] = ["name", "app_id", "compat_tool"];
pub const COLUMN_NAMES: [&str; 3] = ["Game", "App ID", "Compatibility Tool"];

/// A row shown in the games table, built from a discovered [`Game`].
#[derive(Clone, Debug, PartialEq)]
pub struct GameRow {
    pub name: SharedString,
    pub app_id: u32,
    /// The display value of the compatibility tool (see [`display_compat_tool`]).
    pub compat_tool: SharedString,
    pub library_path: String,
    pub proton_dir: String,
}

impl GameRow {
    /// Convert a backend [`Game`] into its table row, applying the display
    /// formatting for the compatibility tool and copying the paths a launch
    /// and the copy actions resolve.
    pub fn from_game(game: &Game) -> Self {
        Self {
            name: game.name.clone().into(),
            app_id: game.app_id,
            compat_tool: display_compat_tool(game).into(),
            library_path: game.library_path.clone(),
            proton_dir: game.proton_dir.clone(),
        }
    }
}

/// The value shown in the "Compatibility Tool" column.
///
/// When Steam has an explicit mapping (per-app `CompatToolMapping`), show that name.
/// Otherwise fall back to a short description of the resolved Proton directory, or a
/// generic "(default)" when nothing is known.
pub fn display_compat_tool(game: &Game) -> String {
    if !game.compat_tool.is_empty() {
        return game.compat_tool.clone();
    }

    if !game.proton_dir.is_empty()
        && let Some(name) = std::path::Path::new(&game.proton_dir)
            .file_name()
            .and_then(|n| n.to_str())
    {
        return format!("{name} (default)");
    }

    "(default)".to_string()
}

/// Sort `rows` by `column` (0 = Game, 1 = App ID, 2 = Compatibility Tool).
///
/// Keys match the legacy model: names and tools compare case-insensitively, and
/// app ids compare numerically (equivalent to Qt's zero-padded string key for a
/// `u32`).
pub fn sort_games(rows: &mut [GameRow], column: usize, ascending: bool) {
    rows.sort_by(|a, b| {
        let order = match column {
            0 => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            1 => a.app_id.cmp(&b.app_id),
            _ => a
                .compat_tool
                .to_lowercase()
                .cmp(&b.compat_tool.to_lowercase()),
        };

        if ascending { order } else { order.reverse() }
    });
}

pub fn find_by_app_id(rows: &[GameRow], app_id: u32) -> Option<usize> {
    rows.iter().position(|row| row.app_id == app_id)
}

/// Build a popup-menu item whose click handler runs on the root app entity.
fn app_menu_item(
    window: &Window,
    entity: &Entity<ProtonctxApp>,
    label: impl Into<SharedString>,
    handler: impl Fn(&mut ProtonctxApp, &mut Window, &mut Context<ProtonctxApp>) + 'static,
) -> PopupMenuItem {
    crate::menu::item(label).on_click(window.listener_for(
        entity,
        move |app, _: &ClickEvent, window, cx| {
            handler(app, window, cx);
        },
    ))
}

pub struct GamesDelegate {
    pub rows: Vec<GameRow>,
    pub name_width: Pixels,
    pub app_id_width: Pixels,
    pub compat_width: Pixels,
    pub selected_row: Option<usize>,
    pub selected_app_id: Option<u32>,
    pub sort_column: usize,
    pub sort_sort: ColumnSort,
    weak_app: WeakEntity<ProtonctxApp>,
}

impl GamesDelegate {
    pub fn new(weak_app: WeakEntity<ProtonctxApp>) -> Self {
        Self {
            rows: Vec::new(),
            name_width: px(260.),
            app_id_width: px(110.),
            compat_width: px(320.),
            selected_row: None,
            selected_app_id: None,
            sort_column: 0,
            sort_sort: ColumnSort::Ascending,
            weak_app,
        }
    }

    /// Replace the rows and clear the mirrored selection, as a discovery
    /// completion or reset does.
    pub fn set_rows(&mut self, rows: Vec<GameRow>) {
        self.rows = rows;
        self.selected_row = None;
        self.selected_app_id = None;
    }

    pub fn sync_selected_row(&mut self, row: Option<usize>) {
        self.selected_row = row;
        self.selected_app_id = row
            .and_then(|row_ix| self.rows.get(row_ix))
            .map(|row| row.app_id);
    }
}

impl TableDelegate for GamesDelegate {
    fn columns_count(&self, _: &App) -> usize {
        COLUMN_KEYS.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        let sort = if col_ix == self.sort_column {
            self.sort_sort
        } else {
            ColumnSort::Default
        };

        Column::new(COLUMN_KEYS[col_ix], COLUMN_NAMES[col_ix])
            .sort(sort)
            .resizable(col_ix < 2)
            .movable(false)
            .selectable(false)
            .min_width(px(40.))
            .width(match col_ix {
                0 => self.name_width,
                1 => self.app_id_width,
                _ => self.compat_width,
            })
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let keep = self.selected_app_id;

        self.sort_column = col_ix;
        self.sort_sort = sort;

        match sort {
            ColumnSort::Ascending => sort_games(&mut self.rows, col_ix, true),
            ColumnSort::Descending => sort_games(&mut self.rows, col_ix, false),
            ColumnSort::Default => {}
        }

        if let Some(app_id) = keep
            && let Some(row_ix) = find_by_app_id(&self.rows, app_id)
        {
            cx.defer_in(window, move |table, _, cx| {
                table.set_selected_row(row_ix, cx);
            });
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().w_full().text_center().child(COLUMN_NAMES[col_ix])
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let selected = self.selected_row == Some(row_ix);

        div()
            .id(("row", row_ix))
            .relative()
            .when(row_ix % 2 == 1, |row| row.bg(cx.theme().tokens.table_even))
            .when(selected && window.is_window_active(), |row| {
                row.child(div().absolute().inset_0().bg(cx.theme().primary))
            })
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text: SharedString = match col_ix {
            0 => self.rows[row_ix].name.clone(),
            1 => self.rows[row_ix].app_id.to_string().into(),
            _ => self.rows[row_ix].compat_tool.clone(),
        };

        div().w_full().overflow_hidden().text_ellipsis().child(text)
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(entity) = self.weak_app.upgrade() else {
            return menu;
        };

        // Resolve the cache path up front so the destructive item is disabled
        // when the game has no cache yet (Qt checks `shaderCachePath(row)`).
        let cache_exists = self.rows.get(row_ix).is_some_and(|row| {
            crate::steam::shadercache::shader_cache_dir_for(
                std::path::Path::new(&row.library_path),
                row.app_id,
            )
            .exists()
        });

        menu.item(app_menu_item(
            window,
            &entity,
            "Browse for executable...",
            |app, window, cx| app.browse_for_executable(window, cx),
        ))
        .separator()
        .item(app_menu_item(
            window,
            &entity,
            "Explorer",
            |app, window, cx| {
                app.launch_tool("explorer", window, cx);
            },
        ))
        .item(app_menu_item(
            window,
            &entity,
            "Registry Editor",
            |app, window, cx| {
                app.launch_tool("regedit", window, cx);
            },
        ))
        .item(app_menu_item(
            window,
            &entity,
            "Task Manager",
            |app, window, cx| {
                app.launch_tool("taskmgr", window, cx);
            },
        ))
        .item(app_menu_item(
            window,
            &entity,
            "Wine Configuration",
            |app, window, cx| {
                app.launch_tool("winecfg", window, cx);
            },
        ))
        .separator()
        .item(app_menu_item(
            window,
            &entity,
            "Copy compatdata path",
            |app, _, cx| app.copy_compat_data_path(cx),
        ))
        .item(app_menu_item(
            window,
            &entity,
            "Copy compatibility tool path",
            |app, _, cx| app.copy_compatibility_tool_path(cx),
        ))
        .separator()
        .item(
            app_menu_item(window, &entity, "Delete Shader Cache", |app, window, cx| {
                app.confirm_delete_shader_cache(window, cx)
            })
            .disabled(!cache_exists),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Game, GameRow, display_compat_tool, find_by_app_id, sort_games};

    fn game(
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

    fn fixtures() -> Vec<GameRow> {
        vec![
            GameRow::from_game(&game(
                "Broforce",
                274190,
                "GE-Proton10-34",
                "/lib",
                "/lib/steamapps/common/GE-Proton10-34",
            )),
            GameRow::from_game(&game(
                "Castle Crashers",
                204360,
                "proton_hotfix",
                "/lib2",
                "/lib2/steamapps/common/Proton Hotfix",
            )),
            GameRow::from_game(&game(
                "Raptor: Call of The Shadows - 2015 Edition",
                336060,
                "proton_experimental",
                "/lib",
                "/lib/steamapps/common/Proton - Experimental",
            )),
        ]
    }

    fn names(rows: &[GameRow]) -> Vec<String> {
        rows.iter().map(|row| row.name.to_string()).collect()
    }

    #[test]
    fn row_conversion_applies_display_tool_and_copies_paths() {
        let source = game(
            "Broforce",
            274190,
            "",
            "/home/u/.local/share/Steam",
            "/home/u/.local/share/Steam/steamapps/common/Proton - Experimental",
        );
        let row = GameRow::from_game(&source);

        assert_eq!(row.name, "Broforce");
        assert_eq!(row.app_id, 274190);
        assert_eq!(row.compat_tool, "Proton - Experimental (default)");
        assert_eq!(row.library_path, source.library_path);
        assert_eq!(row.proton_dir, source.proton_dir);
    }

    #[test]
    fn display_compat_tool_prefers_explicit_mapping() {
        let explicit = game("A", 1, "proton_experimental", "/lib", "/lib/tool");
        assert_eq!(display_compat_tool(&explicit), "proton_experimental");
    }

    #[test]
    fn display_compat_tool_falls_back_to_proton_dir_name() {
        let mapped = game("A", 1, "", "/lib", "/lib/steamapps/common/GE-Proton10-34");
        assert_eq!(display_compat_tool(&mapped), "GE-Proton10-34 (default)");
    }

    #[test]
    fn display_compat_tool_falls_back_to_default() {
        let unknown = game("A", 1, "", "/lib", "");
        assert_eq!(display_compat_tool(&unknown), "(default)");
    }

    #[test]
    fn sorts_by_name_ascending_and_descending() {
        let mut rows = fixtures();

        sort_games(&mut rows, 0, true);
        assert_eq!(
            names(&rows),
            vec![
                "Broforce",
                "Castle Crashers",
                "Raptor: Call of The Shadows - 2015 Edition"
            ]
        );

        sort_games(&mut rows, 0, false);
        assert_eq!(
            names(&rows),
            vec![
                "Raptor: Call of The Shadows - 2015 Edition",
                "Castle Crashers",
                "Broforce"
            ]
        );
    }

    #[test]
    fn sorts_app_id_numerically() {
        let mut rows = fixtures();

        sort_games(&mut rows, 1, true);
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![204360, 274190, 336060]
        );
        assert_eq!(rows[0].name, "Castle Crashers");

        sort_games(&mut rows, 1, false);
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![336060, 274190, 204360]
        );
    }

    #[test]
    fn sorts_compatibility_tool_by_display_value() {
        let mut rows = fixtures();

        sort_games(&mut rows, 2, true);
        assert_eq!(
            rows.iter()
                .map(|row| row.compat_tool.to_string())
                .collect::<Vec<_>>(),
            vec!["GE-Proton10-34", "proton_experimental", "proton_hotfix"]
        );
    }

    #[test]
    fn finds_row_by_app_id() {
        let rows = fixtures();

        assert_eq!(find_by_app_id(&rows, 204360), Some(1));
        assert_eq!(find_by_app_id(&rows, 336060), Some(2));
        assert_eq!(find_by_app_id(&rows, 1), None);
    }
}
