//! Root module: application state, messages, `update`, per-window `view`
//! routing, and subscriptions.
//!
//! The UI is an `iced::daemon`: the main window plus one OS window per open
//! dialog. All mutable state lives in [`State`] and changes only in
//! [`State::update`]; the views in [`crate::views`] are pure functions of it.
//!
//! Discovery loads are serialized, not raced: [`Message::Refresh`] is ignored
//! while `loading` is set, so two scans can never overlap and a completion
//! never needs a stale-generation check.

use iced::widget::{Id, operation, pane_grid, scrollable, text_editor};
use iced::{Size, Subscription, Task, event, keyboard, mouse, window};

use crate::games::{self, ColumnSort, GameRow};
use crate::views::logs_ui::LogState;
use crate::views::table::{self, GameColumn, TableView};
use crate::views::{main_ui, widgets};

/// Main window geometry.
pub const MAIN_SIZE: Size = Size::new(760.0, 520.0);
pub const MAIN_MIN_SIZE: Size = Size::new(468.0, 288.0);

/// Initial split: the table pane takes 420 of 420 + 140 px.
const TABLE_PANE_HEIGHT: f32 = 420.0;
const LOG_PANE_HEIGHT: f32 = 140.0;
/// Per-pane minimums, enforced on the split ratio (`PaneGrid::min_size` is
/// uniform).
pub const TABLE_PANE_MIN: f32 = 160.0;
pub const LOG_PANE_MIN: f32 = 54.0;

/// The two panes of the main split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Table,
    Log,
}

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

/// A runtime event the app listens to, tagged with its window.
#[derive(Clone, Debug)]
pub enum WindowInput {
    Focused,
    Unfocused,
    RightPressed,
    CloseRequested,
    Closed,
    Key {
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    },
}

#[derive(Clone, Debug)]
pub enum Message {
    /// A runtime event for one window.
    Window(window::Id, WindowInput),

    // ---- Toolbar ----
    /// The hamburger's own press; the menu bar opens the menu.
    MenuOpened,
    Refresh,
    GamesLoaded(LoadOutcome),
    ClearLogs,
    OpenSettings,
    OpenAbout,
    Exit,

    // ---- Table ----
    HeaderPressed(usize),
    RowPressed(usize),
    CellEntered(usize, usize),
    CellExited(usize, usize),
    ColumnDragged(usize, f32),
    ColumnReleased,
    TableSynced(scrollable::AbsoluteOffset),
    TableResized(Size),

    // ---- Split and log ----
    PaneResized(pane_grid::ResizeEvent),
    SplitResized(Size),
    LogAction(text_editor::Action),
    LogScrolled(scrollable::Viewport),
    LogResized(Size),
}

pub struct State {
    pub main_window: window::Id,
    /// Whether the main window has keyboard focus; the selected row is only
    /// filled while it does.
    pub main_focused: bool,

    // ---- Games table ----
    pub rows: Vec<GameRow>,
    pub selected: Option<usize>,
    /// The cell under the pointer, `(row, column)`; the row is the hovered
    /// row.
    pub hovered: Option<(usize, usize)>,
    pub sort_column: usize,
    pub sort: ColumnSort,
    pub columns: [GameColumn; 3],
    pub table_header: Id,
    pub table_body: Id,
    /// Inner width of the table frame, measured by a sensor.
    pub table_width: Option<f32>,
    /// Key of the elided texts held by the columns: rebuilt when the rows
    /// or the column widths change.
    display_key: Option<(u64, [u32; 3])>,
    rows_generation: u64,
    /// Whether the selected game has a shader cache directory (the context
    /// menu's Delete Shader Cache item).
    pub selected_cache_exists: bool,

    // ---- Split ----
    pub panes: pane_grid::State<PaneKind>,
    pub split: Option<pane_grid::Split>,
    /// The table pane's share of the split.
    pub split_ratio: f32,
    pub split_height: Option<f32>,

    // ---- Log ----
    pub log: LogState,

    // ---- Status ----
    pub status_text: String,
    pub loading: bool,
    pub remember_last_dir: bool,
    pub launch_running: bool,

    pub theme: iced::Theme,
}

impl State {
    /// The initial state and the task that opens the main window and starts
    /// the first discovery.
    pub fn new(theme: iced::Theme) -> (Self, Task<Message>) {
        let (main_window, open) = window::open(main_window_settings());
        let mut state = Self::with_main_window(main_window);
        state.theme = theme;
        let refresh = state.refresh();
        (state, Task::batch([open.discard(), refresh]))
    }

    /// The state for a main window that already has `main_window` as its id.
    /// Tests use it directly; nothing here touches a real window.
    pub fn with_main_window(main_window: window::Id) -> Self {
        let split_ratio = TABLE_PANE_HEIGHT / (TABLE_PANE_HEIGHT + LOG_PANE_HEIGHT);
        let configuration = pane_grid::Configuration::Split {
            axis: pane_grid::Axis::Horizontal,
            ratio: split_ratio,
            a: Box::new(pane_grid::Configuration::Pane(PaneKind::Table)),
            b: Box::new(pane_grid::Configuration::Pane(PaneKind::Log)),
        };
        let panes = pane_grid::State::with_configuration(configuration);
        let split = panes.layout().splits().next().copied();

        let mut state = Self {
            main_window,
            main_focused: true,
            rows: Vec::new(),
            selected: None,
            hovered: None,
            sort_column: 0,
            sort: ColumnSort::Ascending,
            columns: table::initial_columns(),
            table_header: Id::unique(),
            table_body: Id::unique(),
            table_width: None,
            display_key: None,
            rows_generation: 0,
            selected_cache_exists: false,
            panes,
            split,
            split_ratio,
            split_height: None,
            log: LogState::new(),
            status_text: "Ready".to_string(),
            loading: false,
            remember_last_dir: crate::config::load_config().remember_last_dir,
            launch_running: false,
            theme: iced::Theme::Dark,
        };
        state.sync_table();
        state
    }

    pub fn title(&self, window: window::Id) -> String {
        if window == self.main_window {
            format!("protonctx v{}", env!("CARGO_PKG_VERSION"))
        } else {
            String::new()
        }
    }

    pub fn theme(&self, _window: window::Id) -> Option<iced::Theme> {
        Some(self.theme.clone())
    }

    pub fn view(&self, window: window::Id) -> iced::Element<'_, Message> {
        if window == self.main_window {
            main_ui::view(self)
        } else {
            iced::widget::space().into()
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(window_input).map(|(window, input)| Message::Window(window, input))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.apply(message);
        self.sync_table();
        task
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Window(window, input) => self.on_window_input(window, input),

            Message::MenuOpened => Task::none(),
            Message::Refresh => self.refresh(),
            Message::GamesLoaded(outcome) => self.games_loaded(outcome),
            Message::ClearLogs => {
                self.log.clear();
                Task::none()
            }
            Message::OpenSettings | Message::OpenAbout => Task::none(),
            Message::Exit => window::close(self.main_window),

            Message::HeaderPressed(column) => {
                self.sort_by_header(column);
                Task::none()
            }
            Message::RowPressed(row) => {
                self.select_row(Some(row));
                Task::none()
            }
            Message::CellEntered(row, column) => {
                self.hovered = Some((row, column));
                Task::none()
            }
            Message::CellExited(row, column) => {
                // Moving between cells exits one and enters the next in
                // either order; only clear the hover the exited cell owns.
                if self.hovered == Some((row, column)) {
                    self.hovered = None;
                }
                Task::none()
            }
            Message::ColumnDragged(column, offset) => {
                if let Some(column) = self.columns.get_mut(column)
                    && table::is_resizable(column.index)
                {
                    column.resize_offset = Some(offset);
                }
                Task::none()
            }
            Message::ColumnReleased => {
                for column in &mut self.columns {
                    if let Some(offset) = column.resize_offset.take() {
                        column.width = (column.width + offset).max(table::MIN_COLUMN_WIDTH);
                    }
                }
                self.fit_compat_column();
                Task::none()
            }
            Message::TableSynced(offset) => {
                operation::scroll_to(self.table_header.clone(), offset)
            }
            Message::TableResized(size) => {
                self.table_width = Some(size.width);
                self.fit_compat_column();
                Task::none()
            }

            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.split_ratio = self.clamped_ratio(ratio);
                self.panes.resize(split, self.split_ratio);
                Task::none()
            }
            Message::SplitResized(size) => {
                self.split_height = Some(size.height);
                self.split_ratio = self.clamped_ratio(self.split_ratio);
                if let Some(split) = self.split {
                    self.panes.resize(split, self.split_ratio);
                }
                Task::none()
            }
            Message::LogAction(action) => {
                self.log.perform(action);
                Task::none()
            }
            Message::LogScrolled(viewport) => {
                self.log.scrolled(viewport);
                Task::none()
            }
            Message::LogResized(size) => {
                self.log.resized(size);
                Task::none()
            }
        }
    }

    fn on_window_input(&mut self, window: window::Id, input: WindowInput) -> Task<Message> {
        let is_main = window == self.main_window;
        match input {
            WindowInput::Focused if is_main => self.main_focused = true,
            WindowInput::Unfocused if is_main => self.main_focused = false,
            WindowInput::RightPressed if is_main => {
                // Right-clicking selects the row so the context menu acts on
                // it (Qt `setCurrentIndex`). The context menu itself opens
                // from the cell under the pointer.
                if let Some((row, _)) = self.hovered {
                    self.select_row(Some(row));
                }
            }
            WindowInput::Closed if is_main => return iced::exit(),
            _ => {}
        }
        Task::none()
    }

    /// Re-run Steam discovery, preserving the table's current sort order.
    /// Ignored while a scan is already in flight.
    pub fn refresh(&mut self) -> Task<Message> {
        if self.loading {
            return Task::none();
        }
        self.loading = true;
        self.status_text = "Loading games...".to_string();
        let mut tasks = vec![self.append_log("Refreshing games...")];
        // Surface the sandbox mode: inside a Flatpak, launches are routed
        // through `flatpak-spawn --host`.
        if crate::flatpak::running_in_flatpak() {
            tasks.push(self.append_log("Flatpak environment detected"));
        }
        // Discovery is an unbounded filesystem scan; it runs on the
        // executor's thread pool, off the UI thread.
        tasks.push(Task::perform(
            async { LoadOutcome::from_result(crate::steam::discover_games()) },
            Message::GamesLoaded,
        ));
        Task::batch(tasks)
    }

    fn games_loaded(&mut self, outcome: LoadOutcome) -> Task<Message> {
        self.loading = false;
        // The sort state is read at completion time, so a header click during
        // the scan wins.
        let (rows, status, warnings) = discovery_outcome(outcome, self.sort_column, self.sort);
        let tasks: Vec<_> = warnings
            .into_iter()
            .map(|warning| self.append_log(warning))
            .collect();
        // Replace the rows and clear the selection together, so the action
        // buttons and the status bar never show a stale row.
        self.set_rows(rows);
        self.status_text = status;
        Task::batch(tasks)
    }

    /// Append a timestamped line to the log pane.
    pub fn append_log(&mut self, message: impl Into<String>) -> Task<Message> {
        self.log.append(&message.into())
    }

    /// Replace the rows and clear the selection, as a discovery completion
    /// does.
    pub fn set_rows(&mut self, rows: Vec<GameRow>) {
        self.rows = rows;
        self.rows_generation = self.rows_generation.wrapping_add(1);
        self.hovered = None;
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

    pub fn selected_row(&self) -> Option<&GameRow> {
        self.rows.get(self.selected?)
    }

    /// A header click: the active column toggles, a new column starts
    /// ascending (Qt default). The selection follows its game.
    fn sort_by_header(&mut self, column: usize) {
        let next = if column == self.sort_column {
            games::next_sort(self.sort)
        } else {
            ColumnSort::Ascending
        };
        let keep = self.selected_row().map(|row| row.app_id);
        self.sort_column = column;
        self.sort = next;
        games::sort_games_by(&mut self.rows, column, next);
        self.rows_generation = self.rows_generation.wrapping_add(1);
        self.hovered = None;
        let restored = keep.and_then(|app_id| games::find_by_app_id(&self.rows, app_id));
        self.select_row(restored);
    }

    /// Size the flexible compat column to the rest of the table width.
    fn fit_compat_column(&mut self) {
        let Some(width) = self.table_width else {
            return;
        };
        let fixed: f32 = self.columns[..2].iter().map(|column| column.width).sum();
        self.columns[2].width = table::compat_width(width, fixed);
    }

    /// `ratio` clamped so both panes keep their minimum heights.
    fn clamped_ratio(&self, ratio: f32) -> f32 {
        let Some(height) = self.split_height else {
            return ratio;
        };
        let total = (height - main_ui::SPLIT_GAP).max(1.0);
        let min = (TABLE_PANE_MIN / total).min(1.0);
        let max = (1.0 - LOG_PANE_MIN / total).max(min);
        ratio.clamp(min, max)
    }

    /// Mirror the view-relevant state into the table columns and rebuild the
    /// elided texts when the rows or widths changed. Runs after every update,
    /// the only path that mutates either.
    fn sync_table(&mut self) {
        let view = TableView {
            sort_column: self.sort_column,
            sort: self.sort,
            selected: self.selected,
            hovered: self.hovered.map(|(row, _)| row),
            focused: self.main_focused,
            cache_exists: self.selected_cache_exists,
        };
        for column in &mut self.columns {
            column.view = view;
        }

        let widths = self
            .columns
            .each_ref()
            .map(|column| column.current_width().to_bits());
        let key = (self.rows_generation, widths);
        if self.display_key == Some(key) {
            return;
        }
        self.display_key = Some(key);
        let t = crate::theme::tokens();
        for column in &mut self.columns {
            let room = (column.current_width() - 2.0 * table::CELL_PADDING_X).max(0.0);
            column.texts = self
                .rows
                .iter()
                .map(|row| widgets::elide(&row.cell_text(column.index), room, t.font, t.font_size))
                .collect();
        }
    }
}

/// The main window: 760×520, centred, server-side decorations, the desktop
/// file's app id so the compositor resolves the icon.
fn main_window_settings() -> window::Settings {
    window::Settings {
        size: MAIN_SIZE,
        min_size: Some(MAIN_MIN_SIZE),
        position: window::Position::Centered,
        decorations: true,
        icon: crate::assets::window_icon(),
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::flatpak::window_app_id(),
            ..Default::default()
        },
        ..window::Settings::default()
    }
}

/// The runtime events the app reacts to; a plain `fn` as `listen_with`
/// requires.
fn window_input(
    event: iced::Event,
    _status: event::Status,
    window: window::Id,
) -> Option<(window::Id, WindowInput)> {
    let input = match event {
        iced::Event::Window(window::Event::Focused) => WindowInput::Focused,
        iced::Event::Window(window::Event::Unfocused) => WindowInput::Unfocused,
        iced::Event::Window(window::Event::CloseRequested) => WindowInput::CloseRequested,
        iced::Event::Window(window::Event::Closed) => WindowInput::Closed,
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
            WindowInput::RightPressed
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            WindowInput::Key { key, modifiers }
        }
        _ => return None,
    };
    Some((window, input))
}
