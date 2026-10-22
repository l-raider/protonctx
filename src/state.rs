//! The application state, independent of the UI toolkit.
//!
//! [`crate::app`] owns a [`State`], forwards user actions to it, and pushes
//! the result to the Slint windows; everything here is plain data and pure
//! transitions, unit-tested beside the module.

use crate::games::{self, ColumnSort, GameRow};
use crate::log_buffer::LogBuffer;

/// What a discovery scan produced, already mapped to displayable rows and
/// messages (the backend error is not `Clone`).
#[derive(Clone, Debug, PartialEq)]
pub enum LoadOutcome {
    Found {
        rows: Vec<GameRow>,
        warnings: Vec<String>,
    },
    SteamNotFound,
    Failed(String),
}

impl LoadOutcome {
    pub fn from_result(result: Result<crate::steam::Discovery, crate::steam::SteamError>) -> Self {
        match result {
            Ok(discovery) => Self::Found {
                rows: discovery.games.iter().map(GameRow::from_game).collect(),
                warnings: discovery.warnings,
            },
            Err(crate::steam::SteamError::SteamNotFound) => Self::SteamNotFound,
            Err(error) => Self::Failed(format!("Failed to discover games: {error}")),
        }
    }
}

/// Map a discovery outcome to rows, status text, and log warnings, sorting the
/// rows with the caller's current sort state (Qt's `load_games` contract).
pub fn discovery_outcome(
    outcome: LoadOutcome,
    sort_column: usize,
    sort: ColumnSort,
) -> (Vec<GameRow>, String, Vec<String>) {
    match outcome {
        LoadOutcome::Found { mut rows, warnings } => {
            games::sort_games_by(&mut rows, sort_column, sort);
            let status = match rows.len() {
                0 => "No games found".to_string(),
                1 => "1 game loaded".to_string(),
                n => format!("{n} games loaded"),
            };
            (rows, status, warnings)
        }
        LoadOutcome::SteamNotFound => (Vec::new(), "Steam not found".to_string(), Vec::new()),
        LoadOutcome::Failed(message) => (Vec::new(), message.clone(), vec![message]),
    }
}

#[derive(Debug)]
pub struct State {
    pub rows: Vec<GameRow>,
    pub selected: Option<usize>,
    pub sort_column: usize,
    pub sort: ColumnSort,
    /// Whether the selected game has a shader cache directory (the context
    /// menu's Delete Shader Cache item).
    pub selected_cache_exists: bool,

    pub log: LogBuffer,
    pub status_text: String,
    /// Single-flight flag for discovery.
    pub loading: bool,
    pub remember_last_dir: bool,
    /// Single-flight flag for the portal file chooser.
    pub browsing: bool,
    /// Single-flight flag for the shader-cache deletion.
    pub deleting_shader_cache: bool,

    pub launch_running: bool,
    pub active_launches: usize,
    /// The newest launch owns the status text.
    pub launch_generation: u64,
}

impl State {
    pub fn new(remember_last_dir: bool) -> Self {
        Self {
            rows: Vec::new(),
            selected: None,
            sort_column: 0,
            sort: ColumnSort::Ascending,
            selected_cache_exists: false,
            log: LogBuffer::default(),
            status_text: "Ready".to_string(),
            loading: false,
            remember_last_dir,
            browsing: false,
            deleting_shader_cache: false,
            launch_running: false,
            active_launches: 0,
            launch_generation: 0,
        }
    }

    pub fn selected_row(&self) -> Option<&GameRow> {
        self.rows.get(self.selected?)
    }

    /// Replace the rows and clear the selection, as a discovery completion
    /// does.
    pub fn set_rows(&mut self, rows: Vec<GameRow>) {
        self.rows = rows;
        self.select_row(None);
    }

    pub fn select_row(&mut self, row: Option<usize>) {
        self.selected = row.filter(|row| *row < self.rows.len());
        self.selected_cache_exists = self.selected_row().is_some_and(|row| {
            crate::steam::shadercache::shader_cache_dir_for(
                std::path::Path::new(&row.library_path),
                row.app_id,
            )
            .exists()
        });
    }

    /// A header click: the active column toggles, a new column starts
    /// ascending (Qt default). The selection follows its game.
    pub fn sort_by_header(&mut self, column: usize) {
        let next = if column == self.sort_column {
            games::next_sort(self.sort)
        } else {
            ColumnSort::Ascending
        };
        let keep = self.selected_row().map(|row| row.app_id);
        self.sort_column = column;
        self.sort = next;
        games::sort_games_by(&mut self.rows, column, next);
        let restored = keep.and_then(|app_id| games::find_by_app_id(&self.rows, app_id));
        self.select_row(restored);
    }

    /// Start a discovery. Returns false (and changes nothing) while one is
    /// already in flight.
    pub fn begin_refresh(&mut self) -> bool {
        if self.loading {
            return false;
        }
        self.loading = true;
        self.status_text = "Loading games...".to_string();
        self.log.append("Refreshing games...");
        // Surface the sandbox mode: inside a Flatpak, launches are routed
        // through `flatpak-spawn --host`.
        if crate::flatpak::running_in_flatpak() {
            self.log.append("Flatpak environment detected");
        }
        true
    }

    /// Apply a discovery result. The sort state is read now, so a header
    /// click during the scan wins.
    pub fn games_loaded(&mut self, outcome: LoadOutcome) {
        self.loading = false;
        let (rows, status, warnings) = discovery_outcome(outcome, self.sort_column, self.sort);
        for warning in &warnings {
            self.log.append(warning);
        }
        self.set_rows(rows);
        self.status_text = status;
    }

    /// Record a started launch; returns its generation.
    pub fn launch_started(&mut self, status: String, command_line: &str) -> u64 {
        self.launch_generation = self.launch_generation.wrapping_add(1);
        self.active_launches = self.active_launches.saturating_add(1);
        self.launch_running = true;
        self.status_text = status;
        self.log.append(&format!("Starting {command_line}"));
        self.launch_generation
    }

    /// Apply one launch completion: count it down, log `message`, and let the
    /// newest launch own the status text.
    pub fn finish_launch(&mut self, generation: u64, message: String) {
        self.active_launches = self.active_launches.saturating_sub(1);
        self.launch_running = self.active_launches > 0;
        self.log.append(&message);
        // A newer launch superseded this one for the status text; every
        // process still logs its own exit.
        if generation == self.launch_generation {
            self.status_text = message;
        }
    }

    /// Clear the launch bookkeeping when the exit-watcher thread could not be
    /// spawned.
    pub fn watcher_spawn_failed(&mut self, error: &std::io::Error, generation: u64) {
        self.finish_launch(
            generation,
            format!("Launch error: failed to start watcher: {error}"),
        );
    }

    /// The shader-cache deletion finished; returns the error to show, if any.
    pub fn shader_cache_deleted(
        &mut self,
        game: &GameRow,
        result: Result<bool, String>,
    ) -> Option<String> {
        self.deleting_shader_cache = false;
        // The cache may be gone now, which disables the context-menu item.
        self.select_row(self.selected);
        match result {
            Ok(true) => {
                self.status_text = "Shader cache deleted".to_string();
                self.log.append(&format!(
                    "Deleted shader cache for {} (app id {})",
                    game.name, game.app_id
                ));
                None
            }
            Ok(false) => {
                self.status_text = "No shader cache to delete".to_string();
                self.log.append(&format!(
                    "No shader cache found for {} (app id {})",
                    game.name, game.app_id
                ));
                None
            }
            Err(error) => Some(format!("Failed to delete shader cache: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadOutcome, State, discovery_outcome};
    use crate::games::{ColumnSort, GameRow};
    use crate::test_support::test_game;

    fn state_with(names: &[(&str, u32)]) -> State {
        let mut state = State::new(true);
        state.set_rows(
            names
                .iter()
                .map(|(name, id)| GameRow::from_game(&test_game(name, *id)))
                .collect(),
        );
        state
    }

    #[test]
    fn discovery_outcome_sorts_with_the_current_state_and_maps_errors() {
        let discovery = crate::steam::Discovery {
            games: vec![
                test_game("Castle Crashers", 204360),
                test_game("Broforce", 274190),
            ],
            warnings: vec!["a warning".to_string()],
        };
        let (rows, status, warnings) = discovery_outcome(
            LoadOutcome::from_result(Ok(discovery)),
            1,
            ColumnSort::Descending,
        );
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![274190, 204360]
        );
        assert_eq!(status, "2 games loaded");
        assert_eq!(warnings, vec!["a warning".to_string()]);

        let (rows, status, _) = discovery_outcome(
            LoadOutcome::from_result(Err(crate::steam::SteamError::SteamNotFound)),
            0,
            ColumnSort::Ascending,
        );
        assert!(rows.is_empty());
        assert_eq!(status, "Steam not found");

        let error = crate::steam::SteamError::Parse("bad vdf".to_string());
        let (_, status, warnings) = discovery_outcome(
            LoadOutcome::from_result(Err(error)),
            0,
            ColumnSort::Ascending,
        );
        assert_eq!(status, "Failed to discover games: parse error: bad vdf");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn refresh_is_single_flight_and_applies_the_current_sort() {
        let mut state = State::new(true);
        assert!(state.begin_refresh());
        assert!(!state.begin_refresh());
        assert_eq!(state.status_text, "Loading games...");
        assert_eq!(state.log.text().matches("Refreshing games...").count(), 1);

        state.sort_by_header(1);
        state.games_loaded(LoadOutcome::Found {
            rows: vec![
                GameRow::from_game(&test_game("B", 2)),
                GameRow::from_game(&test_game("A", 1)),
            ],
            warnings: vec!["prefix unreadable".to_string()],
        });
        assert!(!state.loading);
        assert_eq!(state.status_text, "2 games loaded");
        assert_eq!(
            state.rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(state.log.text().contains("] prefix unreadable\n"));
    }

    #[test]
    fn header_click_toggles_two_state_sort() {
        let mut state = state_with(&[("Broforce", 274190), ("Castle Crashers", 204360)]);
        let mut click = |column, sort, ids: &[u32]| {
            state.sort_by_header(column);
            assert_eq!(state.sort_column, column);
            assert_eq!(state.sort, sort);
            let actual: Vec<u32> = state.rows.iter().map(|row| row.app_id).collect();
            assert_eq!(actual, ids);
        };
        click(0, ColumnSort::Descending, &[204360, 274190]);
        click(0, ColumnSort::Ascending, &[274190, 204360]);
        click(1, ColumnSort::Ascending, &[204360, 274190]);
        click(1, ColumnSort::Descending, &[274190, 204360]);
        click(0, ColumnSort::Ascending, &[274190, 204360]);
    }

    #[test]
    fn sort_restores_the_selection_by_app_id() {
        let mut state = state_with(&[("Alpha", 30), ("Beta", 10)]);
        state.select_row(Some(0));
        state.sort_by_header(1);
        assert_eq!(state.selected, Some(1));
        assert_eq!(state.selected_row().map(|row| row.app_id), Some(30));
    }

    #[test]
    fn newest_launch_owns_the_status_text_and_watcher_failure_cleans_up() {
        let mut state = State::new(true);
        let first = state.launch_started("Running winecfg...".to_string(), "proton winecfg");
        let second = state.launch_started("Running regedit...".to_string(), "proton regedit");
        state.finish_launch(first, "Exit code: 0".to_string());
        assert_eq!(state.status_text, "Running regedit...");
        assert!(state.launch_running);
        state.watcher_spawn_failed(&std::io::Error::other("no threads"), second);
        assert!(!state.launch_running);
        assert_eq!(state.active_launches, 0);
        assert!(state.status_text.contains("failed to start watcher"));
        assert!(state.log.text().contains("Starting proton winecfg"));
    }

    #[test]
    fn shader_cache_outcomes_are_reported() {
        let mut state = state_with(&[("Cache Test", 274190)]);
        let game = state.rows[0].clone();
        state.deleting_shader_cache = true;
        assert_eq!(state.shader_cache_deleted(&game, Ok(true)), None);
        assert!(!state.deleting_shader_cache);
        assert_eq!(state.status_text, "Shader cache deleted");
        assert!(
            state
                .log
                .text()
                .contains("Deleted shader cache for Cache Test (app id 274190)")
        );
        assert_eq!(state.shader_cache_deleted(&game, Ok(false)), None);
        assert_eq!(state.status_text, "No shader cache to delete");
        assert_eq!(
            state.shader_cache_deleted(&game, Err("denied".to_string())),
            Some("Failed to delete shader cache: denied".to_string())
        );
    }
}
