//! Pure helpers behind the games table.
//!
//! Row conversion, display formatting, sorting, and identity lookup are pure
//! functions unit-tested beside this module. The table keeps the prototype's
//! behaviour: sort state persists across refreshes, a whole-header click
//! toggles ascending/descending with an app-rendered indicator, and selection
//! is restored by App ID across sorts.

use crate::models::Game;

/// The sort state of a column. The live table only ever uses `Ascending` and
/// `Descending`; `Default` is the neutral state that keeps discovery order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColumnSort {
    #[default]
    Default,
    Ascending,
    Descending,
}

/// A row shown in the games table, built from a discovered [`Game`].
#[derive(Clone, Debug, PartialEq)]
pub struct GameRow {
    pub name: String,
    pub app_id: u32,
    /// The display value of the compatibility tool (see [`display_compat_tool`]).
    pub compat_tool: String,
    pub library_path: String,
    pub steam_root: String,
    pub proton_dir: String,
}

impl GameRow {
    /// Convert a backend [`Game`] into its table row, applying the display
    /// formatting for the compatibility tool and copying the paths a launch
    /// and the copy actions resolve.
    pub fn from_game(game: &Game) -> Self {
        Self {
            name: game.name.clone(),
            app_id: game.app_id,
            compat_tool: display_compat_tool(game),
            library_path: game.library_path.clone(),
            steam_root: game.steam_root.clone(),
            proton_dir: game.proton_dir.clone(),
        }
    }

    /// The row as a backend [`Game`]. The launcher only reads `name`,
    /// `app_id`, `library_path`, `steam_root`, and `proton_dir`.
    pub fn to_game(&self) -> Game {
        Game {
            name: self.name.clone(),
            app_id: self.app_id,
            compat_tool: self.compat_tool.clone(),
            library_path: self.library_path.clone(),
            steam_root: self.steam_root.clone(),
            proton_dir: self.proton_dir.clone(),
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
/// `u32`). Keys are computed once per row (`sort_by_cached_key`) and the sort
/// is stable.
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

/// Apply a [`ColumnSort`] state to `rows`; `Default` leaves discovery order.
pub fn sort_games_by(rows: &mut [GameRow], column: usize, sort: ColumnSort) {
    match sort {
        ColumnSort::Ascending => sort_games(rows, column, true),
        ColumnSort::Descending => sort_games(rows, column, false),
        ColumnSort::Default => {}
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

#[cfg(test)]
mod tests {
    use super::{
        ColumnSort, GameRow, display_compat_tool, find_by_app_id, next_sort, sort_games,
        sort_games_by,
    };
    use crate::test_support::game;

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
        assert_eq!(row.steam_root, source.steam_root);
        assert_eq!(row.proton_dir, source.proton_dir);
        assert_eq!(row.to_game().steam_root, source.steam_root);
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
    fn sort_games_by_dispatches_the_column_sort_state() {
        let mut rows = fixtures();

        sort_games_by(&mut rows, 1, ColumnSort::Ascending);
        assert_eq!(fixture_ids(&rows), vec![204360, 274190, 336060]);
        sort_games_by(&mut rows, 1, ColumnSort::Descending);
        assert_eq!(fixture_ids(&rows), vec![336060, 274190, 204360]);

        // `Default` is the neutral header state: discovery order is preserved.
        let mut rows = fixtures();
        let before = names(&rows);
        sort_games_by(&mut rows, 1, ColumnSort::Default);
        assert_eq!(names(&rows), before);
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
