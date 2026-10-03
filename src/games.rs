//! The games table delegate and its pure helpers.
//!
//! Row conversion, display formatting, sorting, and identity lookup are pure
//! functions unit-tested beside this module; [`GamesDelegate`] keeps the
//! prototype's behaviour (sort state persists across refreshes, a whole-header
//! click toggles ascending/descending with an app-rendered indicator, selection
//! is restored by App ID across sorts, the context menu drives real actions).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    menu::{PopupMenu, PopupMenuItem},
    table::{Column, ColumnSort, TableDelegate, TableState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::models::Game;
use crate::views::main_ui::ProtonctxApp;

pub const COLUMN_KEYS: [&str; 3] = ["name", "app_id", "compat_tool"];
pub const COLUMN_NAMES: [&str; 3] = ["Game", "App ID", "Compatibility Tool"];

/// Built-in Wine tools shared by the action row and the row context menu:
/// `(button id, label, tool id passed to the launcher)`.
pub const TOOL_BUTTONS: [(&str, &str, &str); 4] = [
    ("explorer", "Explorer", "explorer"),
    ("registry-editor", "Registry Editor", "regedit"),
    ("task-manager", "Task Manager", "taskmgr"),
    ("wine-configuration", "Wine Configuration", "winecfg"),
];

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
/// `u32`). Keys are computed once per row (`sort_by_cached_key`), so the
/// case-insensitive columns no longer allocate two strings per comparison; the
/// sort stays stable, exactly like the previous comparator.
pub fn sort_games(rows: &mut [GameRow], column: usize, ascending: bool) {
    match (column, ascending) {
        (0, true) => rows.sort_by_cached_key(|row| row.name.to_lowercase()),
        (0, false) => rows.sort_by_cached_key(|row| std::cmp::Reverse(row.name.to_lowercase())),
        (1, true) => rows.sort_by_cached_key(|row| row.app_id),
        (1, false) => rows.sort_by_cached_key(|row| std::cmp::Reverse(row.app_id)),
        (_, true) => rows.sort_by_cached_key(|row| row.compat_tool.to_lowercase()),
        (_, false) => {
            rows.sort_by_cached_key(|row| std::cmp::Reverse(row.compat_tool.to_lowercase()))
        }
    }
}

/// Next sort direction when the active column's header is clicked.
///
/// Two-state toggle, matching Qt's default header behavior
/// (`Ascending <-> Descending`). `Default` maps to `Ascending` defensively;
/// the live table never reports it.
pub fn next_sort(current: ColumnSort) -> ColumnSort {
    match current {
        ColumnSort::Ascending => ColumnSort::Descending,
        _ => ColumnSort::Ascending,
    }
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

    /// Header sort indicator: a solid arrow on the sorted column, an invisible
    /// same-size placeholder elsewhere so centered text does not shift when the
    /// sorted column changes.
    fn sort_icon(&self, col_ix: usize, cx: &App) -> Stateful<Div> {
        // The id is required for the hover/active tint state, not for input:
        // no click listener here, so clicks bubble to the header handler.
        let slot = div()
            .id(("sort-icon", col_ix))
            .p(px(2.))
            .flex_shrink_0()
            .rounded(cx.theme().radius / 2.);

        if col_ix == self.sort_column {
            let icon = match self.sort_sort {
                ColumnSort::Descending => IconName::SortDescending,
                _ => IconName::SortAscending,
            };
            slot.hover(|this| this.bg(cx.theme().tokens.secondary).opacity(7.))
                .active(|this| this.bg(cx.theme().accent).opacity(1.))
                .child(
                    Icon::new(icon)
                        .size_3()
                        .text_color(cx.theme().secondary_foreground),
                )
        } else {
            slot.child(Icon::new(IconName::ChevronsUpDown).size_3().opacity(0.))
        }
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
        // Without `.sort(...)` the library's tri-state icon and its private
        // `perform_sort` cycle never run; `render_th` owns the indicator.
        Column::new(COLUMN_KEYS[col_ix], COLUMN_NAMES[col_ix])
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
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .id(("th", col_ix))
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .on_click(cx.listener(
                move |table: &mut TableState<GamesDelegate>, _: &ClickEvent, window, cx| {
                    // Whole-header click, like Qt's QHeaderView. The indicator is a
                    // child of this element and has no listener, so one click is one
                    // sort regardless of where in the header it lands.
                    if !table.sortable {
                        return;
                    }

                    let next = {
                        let delegate = table.delegate();
                        if col_ix == delegate.sort_column {
                            next_sort(delegate.sort_sort)
                        } else {
                            // A newly clicked column starts ascending (Qt default).
                            ColumnSort::Ascending
                        }
                    };

                    table.delegate_mut().perform_sort(col_ix, next, window, cx);
                    // `refresh` keeps cached column metadata in sync; `notify` is
                    // what repaints, so `render_th` re-reads the sort state.
                    table.refresh(cx);
                    cx.stop_propagation();
                    cx.notify();
                },
            ))
            .child(div().flex_1().text_center().child(COLUMN_NAMES[col_ix]))
            .child(self.sort_icon(col_ix, cx))
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

        let mut menu = menu
            .item(app_menu_item(
                window,
                &entity,
                "Browse for executable...",
                |app, window, cx| app.browse_for_executable(window, cx),
            ))
            .separator();

        // Build the tool items from the shared table so the toolbar and this
        // menu cannot drift apart.
        for (_, label, tool) in TOOL_BUTTONS {
            menu = menu.item(app_menu_item(
                window,
                &entity,
                label,
                move |app, window, cx| {
                    app.launch_tool(tool, window, cx);
                },
            ));
        }

        menu.separator()
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
    use super::{
        ColumnSort, Game, GameRow, display_compat_tool, find_by_app_id, next_sort, sort_games,
    };

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

    /// Mixed-case names/tools and equal keys: the cached-key sort must match
    /// the old comparator exactly, including stable tie order in both
    /// directions.
    fn case_insensitive_fixtures() -> Vec<GameRow> {
        vec![
            GameRow::from_game(&game("alpha", 30, "Proton-GE", "/lib", "")),
            GameRow::from_game(&game("ALPHA", 10, "proton-ge", "/lib", "")),
            GameRow::from_game(&game("beta", 40, "Proton", "/lib", "")),
            GameRow::from_game(&game("Beta", 20, "proton", "/lib", "")),
        ]
    }

    fn fixture_ids(rows: &[GameRow]) -> Vec<u32> {
        rows.iter().map(|row| row.app_id).collect()
    }

    #[test]
    fn sorts_case_insensitively_and_stably_with_cached_keys() {
        // Name: "alpha"/"ALPHA" tie (original order), then "beta"/"Beta".
        let mut rows = case_insensitive_fixtures();
        sort_games(&mut rows, 0, true);
        assert_eq!(fixture_ids(&rows), vec![30, 10, 40, 20]);
        sort_games(&mut rows, 0, false);
        assert_eq!(fixture_ids(&rows), vec![40, 20, 30, 10]);

        // App ID: unique keys, no tie ambiguity.
        let mut rows = case_insensitive_fixtures();
        sort_games(&mut rows, 1, true);
        assert_eq!(fixture_ids(&rows), vec![10, 20, 30, 40]);
        sort_games(&mut rows, 1, false);
        assert_eq!(fixture_ids(&rows), vec![40, 30, 20, 10]);

        // Tool: "Proton"/"proton" tie, then "Proton-GE"/"proton-ge" tie.
        let mut rows = case_insensitive_fixtures();
        sort_games(&mut rows, 2, true);
        assert_eq!(fixture_ids(&rows), vec![40, 20, 30, 10]);
        sort_games(&mut rows, 2, false);
        assert_eq!(fixture_ids(&rows), vec![30, 10, 40, 20]);

        // Columns beyond the known three keep the compatibility-tool key.
        let mut rows = case_insensitive_fixtures();
        sort_games(&mut rows, 9, true);
        assert_eq!(fixture_ids(&rows), vec![40, 20, 30, 10]);
    }

    #[test]
    fn header_sort_toggles_between_ascending_and_descending() {
        assert_eq!(next_sort(ColumnSort::Ascending), ColumnSort::Descending);
        assert_eq!(next_sort(ColumnSort::Descending), ColumnSort::Ascending);
        assert_eq!(next_sort(ColumnSort::Default), ColumnSort::Ascending);
    }

    #[test]
    fn finds_row_by_app_id() {
        let rows = fixtures();

        assert_eq!(find_by_app_id(&rows, 204360), Some(1));
        assert_eq!(find_by_app_id(&rows, 336060), Some(2));
        assert_eq!(find_by_app_id(&rows, 1), None);
    }
}
