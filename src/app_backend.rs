//! cxx-qt bridge exposing the Steam games table as a `QAbstractTableModel`.
//!
//! The model is consumed by `src/qt_main_ui.cpp`, which builds a native Qt Widgets
//! `QMainWindow`. All
//! Steam discovery and Proton launching is delegated to `crate::steam` and
//! `crate::launcher`, which are reused unchanged from the earlier Slint implementation.

// cxx-qt's generated code attaches `#[automatically_derived]` to inherent impl blocks, which newer rustc warns about.
#![allow(unused_attributes)]

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    Orientation, QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant,
};

use crate::launcher;
use crate::models::Game;

// Qt::DisplayRole — the only role this model reports.
const ROLE_DISPLAY: i32 = 0;

/// Column indexes for the games table (Game | App ID | Compatibility Tool).
///
/// `column_count` reports [`column::COUNT`]; adding a column is a single-point
/// edit here plus a new arm in `data`/`header_data`/`sort_key`.
mod column {
    pub const NAME: usize = 0;
    pub const APP_ID: usize = 1;
    pub const COMPAT_TOOL: usize = 2;
    pub const COUNT: usize = 3;
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!("QtCore/QAbstractTableModel");
        #[qobject]
        type QAbstractTableModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qt.h");
        #[namespace = "Qt"]
        type Orientation = cxx_qt_lib::Orientation;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractTableModel]
        #[qproperty(i32, selected_row)]
        #[qproperty(QString, status_text)]
        #[qproperty(QString, app_version)]
        #[qproperty(bool, remember_last_dir)]
        #[qproperty(bool, launch_running)]
        type AppBackend = super::AppBackendRust;

        // QAbstractTableModel overrides.
        #[cxx_override]
        fn data(self: &AppBackend, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &AppBackend, parent: &QModelIndex) -> i32;
        #[cxx_override]
        #[cxx_name = "columnCount"]
        fn column_count(self: &AppBackend, parent: &QModelIndex) -> i32;
        #[cxx_override]
        #[cxx_name = "headerData"]
        fn header_data(
            self: &AppBackend,
            section: i32,
            orientation: Orientation,
            role: i32,
        ) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &AppBackend) -> QHash_i32_QByteArray;

        // Inherited model mutation methods (reset is enough: games are loaded once
        // and re-sorted in place).
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut AppBackend>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut AppBackend>);

        // Error signal emitted when a launch fails; wired to a QMessageBox in C++.
        #[qsignal]
        fn launch_failed(self: Pin<&mut AppBackend>, message: &QString);

        // One log line emitted for every lifecycle event and every captured
        // stdout/stderr line of a launched process; wired to the log panel in C++.
        #[qsignal]
        fn log_line(self: Pin<&mut AppBackend>, message: &QString);

        // Invokables called from the C++ UI.
        #[qinvokable]
        fn load_games(self: Pin<&mut AppBackend>);
        #[qinvokable]
        fn select_row(self: Pin<&mut AppBackend>, row: i32);
        #[qinvokable]
        #[cxx_name = "selectedAppId"]
        fn selected_app_id(self: &AppBackend, row: i32) -> u32;
        #[qinvokable]
        #[cxx_name = "compatDataPath"]
        fn compat_data_path(self: &AppBackend, row: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "protonDirPath"]
        fn proton_dir_path(self: &AppBackend, row: i32) -> QString;
        #[qinvokable]
        fn sort_by(self: Pin<&mut AppBackend>, column: i32, ascending: bool);
        #[qinvokable]
        fn launch_tool(self: Pin<&mut AppBackend>, arg: &QString);
        #[qinvokable]
        fn browse_exe(self: Pin<&mut AppBackend>, path: &QString);
        #[qinvokable]
        #[cxx_name = "lastDir"]
        fn last_dir(self: &AppBackend) -> QString;
        #[qinvokable]
        #[cxx_name = "saveLastDir"]
        fn save_last_dir(self: Pin<&mut AppBackend>, path: &QString);
        #[qinvokable]
        #[cxx_name = "applyRememberLastDir"]
        fn apply_remember_last_dir(self: Pin<&mut AppBackend>, enabled: bool);
    }

    impl cxx_qt::Threading for AppBackend {}
}

/// Rust-side state backing the `AppBackend` qobject.
pub struct AppBackendRust {
    /// Installed Steam games, in display (sorted) order.
    games: Vec<Game>,
    /// Index into `games` of the selected row, or `-1`.
    selected_row: i32,
    /// Status-bar text.
    status_text: QString,
    /// Application version, shown in the About dialog.
    app_version: QString,
    /// Whether the "Browse…" file picker remembers (and reopens at) the last directory.
    remember_last_dir: bool,
    /// Whether a launch is currently in flight (drives the status-bar progress indicator).
    launch_running: bool,
    /// Number of launched-but-not-yet-exited child processes. Kept separate from
    /// `launch_running` so that overlapping launches clear the progress indicator only
    /// after the *last* one finishes.
    active_launches: usize,
    /// Monotonic launch-generation counter used to discard stale watcher results when
    /// multiple launches overlap.
    launch_generation: u64,
    /// Cached `roleNames()` map — roles never change, so build it once.
    role_names: QHash<QHashPair_i32_QByteArray>,
    /// Monotonic load-generation counter used to discard stale async results.
    load_generation: u64,
}

impl Default for AppBackendRust {
    fn default() -> Self {
        let mut role_names = QHash::<QHashPair_i32_QByteArray>::default();
        role_names.insert(ROLE_DISPLAY, QByteArray::from("display"));
        let config = crate::config::load_config();
        Self {
            games: Vec::new(),
            selected_row: -1,
            status_text: QString::from("Ready"),
            app_version: QString::from(env!("CARGO_PKG_VERSION")),
            remember_last_dir: config.remember_last_dir,
            launch_running: false,
            active_launches: 0,
            launch_generation: 0,
            role_names,
            load_generation: 0,
        }
    }
}

impl qobject::AppBackend {
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        self.rust().games.len() as i32
    }

    fn column_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        column::COUNT as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() {
            return QVariant::default();
        }
        let row = index.row() as usize;
        let games = &self.rust().games;
        let Some(game) = games.get(row) else {
            return QVariant::default();
        };

        if role != ROLE_DISPLAY {
            return QVariant::default();
        }

        let text = match index.column() as usize {
            column::NAME => game.name.clone(),
            column::APP_ID => game.app_id.to_string(),
            column::COMPAT_TOOL => display_compat_tool(game),
            _ => return QVariant::default(),
        };
        // Build an owned QString first, then wrap it in a QVariant. Previously
        // `QVariant::from(&QString::from(&text))` bound the QVariant to a
        // temporary, which cxx-qt did not guarantee would outlive the call.
        let qstring = QString::from(&text);
        QVariant::from(&qstring)
    }

    fn header_data(&self, section: i32, orientation: Orientation, role: i32) -> QVariant {
        if role != ROLE_DISPLAY || section < 0 {
            return QVariant::default();
        }

        if orientation == Orientation::Vertical {
            let row = section + 1;
            return QVariant::from(&row);
        }

        let label = match section as usize {
            column::NAME => Some(QString::from("Game")),
            column::APP_ID => Some(QString::from("App ID")),
            column::COMPAT_TOOL => Some(QString::from("Compatibility Tool")),
            _ => None,
        };
        label.map(|text| QVariant::from(&text)).unwrap_or_default()
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        self.rust().role_names.clone()
    }

    fn load_games(mut self: Pin<&mut Self>) {
        // Discovery performs an unbounded FS scan, so run it on a background
        // thread rather than blocking the GUI thread before the event loop runs.
        // Results are delivered back onto the Qt event loop via `CxxQtThread`;
        // the model must only be mutated on the GUI thread.
        //
        // NOTE (lifetime/shutdown): the worker thread is detached. This is safe:
        // `discover_games()` is pure (no references to the qobject or GUI), and
        // the closure only captures `qt_thread` (a `CxxQtThread`, which is
        // `'static`/owned) plus the `generation` integer. On shutdown, returning
        // from `main()` terminates the detached thread; if it has already queued
        // a result, that closure is simply never run because the Qt event loop
        // (`qt_app_exec`) has returned and `queue` is dropped. There is no
        // use-after-free: the qobject lives on the GUI thread, and all mutation
        // is marshalled back to that thread via `queue`.
        self.as_mut()
            .set_status_text(QString::from("Loading games..."));

        // Bump the generation up front, so a stale in-flight result is dropped
        // if `load_games` is invoked again before this scan finishes.
        let generation = {
            let mut state = self.as_mut().rust_mut();
            state.load_generation = state.load_generation.wrapping_add(1);
            state.load_generation
        };

        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let result = crate::steam::discover_games();
            let _ = qt_thread.queue(move |mut app| {
                // A newer load superseded this one while discovery was running;
                // discard the stale result rather than clobbering the latest.
                if generation != app.as_ref().rust().load_generation {
                    return;
                }
                let (games, status) = match result {
                    Ok(mut games) => {
                        // Default view: sorted by game name, ascending (matches
                        // the C++ sort indicator).
                        games.sort_by_key(|game| sort_key(game, column::NAME));
                        let count = games.len();
                        let status = match count {
                            0 => "No games found".to_string(),
                            1 => "1 game loaded".to_string(),
                            n => format!("{n} games loaded"),
                        };
                        (games, status)
                    }
                    Err(crate::steam::SteamError::SteamNotFound) => {
                        (Vec::new(), "Steam not found".to_string())
                    }
                    Err(e) => {
                        let msg = format!("Failed to discover games: {e}");
                        eprintln!("protonctx: {msg}");
                        (Vec::new(), msg)
                    }
                };

                unsafe {
                    app.as_mut().begin_reset_model();
                }
                app.as_mut().rust_mut().games = games;
                unsafe {
                    app.as_mut().end_reset_model();
                }

                app.as_mut().set_selected_row(-1);
                app.as_mut().set_status_text(QString::from(&status));
            });
        });
    }

    fn select_row(mut self: Pin<&mut Self>, row: i32) {
        // Guard the index space: `-1` clears selection, anything >= len is
        // clamped to `-1` so a stale row can never select the wrong game.
        let len = self.rust().games.len() as i32;
        let clamped = if row < -1 || row >= len { -1 } else { row };
        self.as_mut().set_selected_row(clamped);
    }

    /// The Steam App ID at `row`, or `0` when the row is out of range.
    ///
    /// `0` is a "no such row" sentinel: the C++ side only calls this with a
    /// valid selected row, so the sentinel is unreachable in practice.
    fn selected_app_id(&self, row: i32) -> u32 {
        self.rust()
            .games
            .get(row as usize)
            .map(|game| game.app_id)
            .unwrap_or(0)
    }

    /// The absolute path of the game's compatibility (prefix) directory, i.e.
    /// `<library>/steamapps/compatdata/<app_id>`, or an empty string if the row is out of
    /// range. `compatdata` lives under the library the game is installed in (a game on a
    /// secondary library keeps its prefix there, not under the Steam root). Resolved via
    /// the same shared helper the launcher uses ([`launcher::proton::compat_data_dir_for`]),
    /// so the copied path always matches the prefix a launch actually uses.
    fn compat_data_path(&self, row: i32) -> QString {
        let Some(game) = self.rust().games.get(row as usize) else {
            return QString::default();
        };

        let path = crate::launcher::proton::compat_data_dir_for(
            std::path::Path::new(&game.library_path),
            game.app_id,
        );

        QString::from(&path.to_string_lossy().into_owned())
    }

    /// The absolute path of the compatibility tool the game runs with (its `proton_dir`,
    /// e.g. `.../steamapps/common/Proton - Experimental`), or an empty string when it
    /// could not be resolved (the game's prefix has not been created yet).
    fn proton_dir_path(&self, row: i32) -> QString {
        self.rust()
            .games
            .get(row as usize)
            .map(|game| QString::from(&game.proton_dir))
            .unwrap_or_default()
    }

    fn sort_by(mut self: Pin<&mut Self>, column: i32, ascending: bool) {
        let column_count = self.column_count(&QModelIndex::default());
        if column < 0 || column >= column_count {
            return;
        }

        let col = column as usize;
        // Remember which game is selected so it can be re-selected at its new
        // row after the reorder (a model reset clears the view's selection).
        let selected_app_id = self.rust().selected_game().map(|game| game.app_id);

        unsafe {
            self.as_mut().begin_reset_model();
        }
        self.as_mut().rust_mut().games.sort_by(|a, b| {
            let a_val = sort_key(a, col);
            let b_val = sort_key(b, col);
            if ascending {
                a_val.cmp(&b_val)
            } else {
                b_val.cmp(&a_val)
            }
        });
        // Re-select the same game at its new row. This must happen before
        // end_reset_model() so the modelReset handler on the C++ side sees the
        // updated row and can restore the view selection.
        let new_row = selected_app_id
            .and_then(|id| self.rust().games.iter().position(|game| game.app_id == id))
            .map(|row| row as i32)
            .unwrap_or(-1);
        self.as_mut().set_selected_row(new_row);
        unsafe {
            self.as_mut().end_reset_model();
        }
    }

    fn launch_tool(mut self: Pin<&mut Self>, arg: &QString) {
        let Some(game) = self.rust().selected_game() else {
            let msg = QString::from("No game selected");
            self.as_mut().launch_failed(&msg);
            return;
        };

        let tool = arg.to_string();
        match launcher::launch_tool(game, &tool) {
            Ok(proc) => {
                // Log the launch using the full command line (paths + prefix).
                let start = QString::from(&format!("Starting {}", proc.command_line));
                self.as_mut().log_line(&start);
                self.start_launch_watcher(proc, format!("Running {tool}..."));
            }
            Err(e) => {
                let msg = QString::from(&format!("Failed to launch {tool}: {e}"));
                self.as_mut().launch_failed(&msg);
            }
        }
    }

    fn browse_exe(mut self: Pin<&mut Self>, path: &QString) {
        let Some(game) = self.rust().selected_game() else {
            let msg = QString::from("No game selected");
            self.as_mut().launch_failed(&msg);
            return;
        };

        let path = path.to_string();
        match launcher::proton::run_in_prefix(game, &[&path]) {
            Ok(proc) => {
                let start = QString::from(&format!("Starting {}", proc.command_line));
                self.as_mut().log_line(&start);
                self.start_launch_watcher(proc, format!("Running {path}..."));
            }
            Err(e) => {
                let msg = QString::from(&format!("Failed to launch: {e}"));
                self.as_mut().launch_failed(&msg);
            }
        }
    }

    /// Record that a launch is now running and spawn a background thread that waits for
    /// the child process to exit, then clears the running state on the Qt thread.
    ///
    /// The [`LaunchedProcess`] is moved into the watcher thread (its `Child` is not `Sync`, so it
    /// cannot live in the qobject struct). A monotonic generation counter ensures a stale
    /// watcher (from an earlier, still-running launch) cannot clobber the status text that a
    /// newer launch has set, while `active_launches` ensures `launch_running` is cleared only
    /// after the last overlapping launch finishes.
    fn start_launch_watcher(
        mut self: Pin<&mut Self>,
        mut proc: launcher::LaunchedProcess,
        status: String,
    ) {
        // Bump the generation so any previously-spawned watcher becomes stale for
        // status-text purposes, and count this launch as active.
        let generation = {
            let mut state = self.as_mut().rust_mut();
            state.launch_generation = state.launch_generation.wrapping_add(1);
            state.active_launches = state.active_launches.saturating_add(1);
            state.launch_generation
        };

        // Take the piped stdout/stderr out of the child so dedicated reader threads
        // can stream them while the watcher thread blocks on the child's exit.
        let stdout = proc.child.stdout.take();
        let stderr = proc.child.stderr.take();
        let mut child = proc.child;

        self.as_mut().set_launch_running(true);
        self.as_mut().set_status_text(QString::from(&status));

        let qt_thread = self.qt_thread();

        // Stream each captured pipe to the log panel from its own detached thread.
        // `log_pipe()` takes ownership of the reader (and a clone of the thread
        // handle) and forwards each line to the GUI thread via `log_line`.
        if let Some(stdout) = stdout {
            log_pipe(stdout, qt_thread.clone());
        }
        if let Some(stderr) = stderr {
            log_pipe(stderr, qt_thread.clone());
        }

        // NOTE (lifetime/shutdown): the watcher thread is detached and owns the
        // `Child` (which is not `Sync`, so it cannot live in the qobject struct).
        // This is safe: it only calls `child.wait()` (blocking on the child) and
        // then `qt_thread.queue` to marshal the result back to the GUI thread.
        // On shutdown, returning from `main()` terminates the thread; a queued
        // result is simply never delivered once the event loop has returned.
        std::thread::spawn(move || {
            let result = child.wait();
            let _ = qt_thread.queue(move |mut app| {
                let (is_latest, still_running) = {
                    let mut state = app.as_mut().rust_mut();
                    state.active_launches = state.active_launches.saturating_sub(1);
                    // A newer launch superseded this one for *status text*
                    // purposes; do not clobber its message.
                    let is_latest = generation == state.launch_generation;
                    // Clear the running indicator only when no launch is left in
                    // flight.
                    let still_running = state.active_launches > 0;
                    (is_latest, still_running)
                };

                // Each launched process logs its own exit event regardless of
                // latestness, so overlapping launches each get a matching entry.
                let finished = match result {
                    Ok(status) => match status.code() {
                        Some(0) => "Exit code: 0".to_string(),
                        Some(code) => format!("Exit code: {code}"),
                        None => "Process terminated by signal".to_string(),
                    },
                    Err(e) => format!("Launch error: {e}"),
                };
                app.as_mut().log_line(&QString::from(&finished));

                if is_latest {
                    app.as_mut().set_status_text(QString::from(&finished));
                }

                app.as_mut().set_launch_running(still_running);
            });
        });
    }

    /// The remembered last directory (from `state.json`), or an empty string when
    /// none has been recorded. The C++ side seeds the "Browse…" dialog with this.
    fn last_dir(&self) -> QString {
        crate::config::load_last_dir()
            .map(|p| QString::from(&p.to_string_lossy().into_owned()))
            .unwrap_or_default()
    }

    /// Persist the directory the user picked (the parent directory of the selected
    /// file) so the next "Browse…" open starts there. No-op when the feature is off.
    fn save_last_dir(self: Pin<&mut Self>, path: &QString) {
        if !self.rust().remember_last_dir {
            return;
        }
        let dir = std::path::PathBuf::from(path.to_string());
        if let Err(e) = crate::config::save_last_dir(&dir) {
            eprintln!("protonctx: failed to save last directory: {e}");
        }
    }

    /// Toggle the "remember last directory" preference and persist it immediately so
    /// the checkbox state survives restarts even if the app is killed before exit.
    fn apply_remember_last_dir(mut self: Pin<&mut Self>, enabled: bool) {
        self.as_mut().set_remember_last_dir(enabled);
        let config = crate::config::AppConfig {
            remember_last_dir: enabled,
        };
        if let Err(e) = crate::config::save_config(&config) {
            eprintln!("protonctx: failed to save config: {e}");
        }
    }
}

impl AppBackendRust {
    /// The currently selected [`Game`], if any.
    fn selected_game(&self) -> Option<&Game> {
        let idx = self.selected_row;
        if idx < 0 {
            return None;
        }
        self.games.get(idx as usize)
    }
}

/// Stream one captured pipe (stdout or stderr) of a launched child to the log panel.
///
/// Runs on its own detached thread: it reads `reader` line-by-line (blocking until the
/// child closes the pipe) and forwards each line to the GUI thread via `log_line`.
/// On shutdown, returning from `main()` terminates the thread; queued lines are simply
/// never delivered once the event loop has returned.
fn log_pipe<R: std::io::Read + Send + 'static>(
    reader: R,
    qt_thread: cxx_qt::CxxQtThread<qobject::AppBackend>,
) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(reader).lines() {
            let text = match line {
                Ok(text) => text,
                Err(e) => {
                    let _ = qt_thread.queue(move |mut app| {
                        let msg = QString::from(&format!("read error: {e}"));
                        app.as_mut().log_line(&msg);
                    });
                    break;
                }
            };
            let _ = qt_thread.queue(move |mut app| {
                app.as_mut().log_line(&QString::from(&text));
            });
        }
    });
}

/// The sort key for a given column index ([`column::NAME`], [`column::APP_ID`], [`column::COMPAT_TOOL`]).
fn sort_key(game: &Game, column: usize) -> String {
    match column {
        column::NAME => game.name.to_lowercase(),
        column::APP_ID => format!("{:010}", game.app_id),
        column::COMPAT_TOOL => display_compat_tool(game).to_lowercase(),
        _ => String::new(),
    }
}

/// The value shown in the "Compatibility Tool" column.
///
/// When Steam has an explicit mapping (per-app `CompatToolMapping`), show that name.
/// Otherwise fall back to a short description of the resolved Proton directory, or a
/// generic "(default)" when nothing is known.
fn display_compat_tool(game: &Game) -> String {
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
