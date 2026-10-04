//! Root view: toolbar, games table, action row, log pane, and status bar.
//!
//! The view owns the UI state (selection mirror, status text, launch counters)
//! and drives the unchanged backend modules (`steam`, `launcher`, `config`,
//! `flatpak`). State/action handling follows the prototype: subscriptions are
//! stored, async completions run through `update_in`, and handlers are chosen
//! per the lessons' listener table.
//!
//! Discovery loads are serialized, not raced: [`ProtonctxApp::refresh_games`]
//! refuses to start while `loading` is set, so two scans can never overlap and
//! a completion never needs a stale-generation check.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    h_flex,
    input::TextareaState,
    menu::DropdownMenu as _,
    resizable::{resizable_panel, v_resizable},
    spinner::Spinner,
    status_bar::StatusBar,
    table::{ColumnSort, DataTable, TableEvent, TableState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::games::{GameRow, GamesDelegate, TOOL_BUTTONS, sort_games};
use crate::log;
use crate::menu;
use crate::models::Game;
use crate::views::{about_ui, dialog_ui, logs_ui, settings_ui};
use crate::{LogClear, LogCopy, LogSelectAll, RefreshGames};

/// Cap for the log textarea; the oldest lines are trimmed first.
pub const MAX_LOG_LINES: usize = 500;
/// Extra lines allowed before a trim runs, so trimming is not per-line work.
pub const TRIM_SLACK: usize = 50;
/// Backpressure cap for launch output events. The reader threads block when
/// the queue is full, which backpressures the child's pipe; without a bound, a
/// chatty process (or a stalled UI consumer) would grow the queue forever.
const LAUNCH_EVENT_CAPACITY: usize = 1024;

/// Register the global key bindings for the app's actions.
///
/// Shared by `main` and tests so chord dispatch behaves identically in both.
/// The Ctrl chords are not claimed by the focused log textarea: `ctrl-l` is
/// unbound there, and while `ctrl-c`/`ctrl-a` have textarea-native bindings,
/// those produce the same copy/select-all results as the actions.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f5", RefreshGames, None),
        KeyBinding::new("ctrl-c", LogCopy, None),
        KeyBinding::new("ctrl-a", LogSelectAll, None),
        KeyBinding::new("ctrl-l", LogClear, None),
    ]);
}

/// Events sent from the launch reader/watcher threads to the UI task.
enum LaunchEvent {
    /// A captured stdout/stderr line, or a read error from one of the pipes.
    Line { text: String },
    /// A launched process exited (or waiting on it failed).
    Exited {
        generation: u64,
        result: std::io::Result<std::process::ExitStatus>,
    },
}

pub struct ProtonctxApp {
    table: Entity<TableState<GamesDelegate>>,
    log: Entity<TextareaState>,
    log_text: String,
    loading: bool,
    selected_row: Option<usize>,
    status_text: String,
    pub(crate) remember_last_directory: bool,
    launch_running: bool,
    active_launches: usize,
    launch_generation: u64,
    browsing: bool,
    viewport_width: Option<Pixels>,
    focus_handle: FocusHandle,
    /// Single-flight flag for [`schedule_frame`]: true while a next-frame
    /// callback is already queued.
    repaint_queued: Rc<Cell<bool>>,
    _subscriptions: Vec<Subscription>,
    _activation_subscription: Subscription,
    _refresh_task: Option<Task<()>>,
    _launch_events_task: Option<Task<()>>,
    _browse_task: Option<Task<()>>,
    _delete_cache_task: Option<Task<()>>,
    /// True while the shader-cache deletion spawned by the confirm dialog is
    /// in flight; the dialog rebuild reads it to disable its buttons.
    deleting_shader_cache: bool,
    launch_tx: async_channel::Sender<LaunchEvent>,
    launch_rx: async_channel::Receiver<LaunchEvent>,
}

impl ProtonctxApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak_app = cx.weak_entity();
        let delegate = GamesDelegate::new(weak_app);
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .col_resizable(true)
                .row_selectable(true)
                .sortable(true)
        });

        let log = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx);
            // Qt's `QPlainTextEdit::NoWrap`: long lines scroll horizontally
            // instead of wrapping into extra rows.
            state.set_soft_wrap(false, window, cx);
            state
        });

        // Selection mirror + column-width sync (P1/P2: never read the leased
        // table from inside its own events; mirror what the root renders).
        let subscription = cx.subscribe_in(
            &table,
            window,
            |this, state, event: &TableEvent, window, cx| match event {
                TableEvent::SelectRow(row) => {
                    let row = *row;
                    this.selected_row = Some(row);
                    state.update(cx, |table, _| {
                        table.delegate_mut().sync_selected_row(Some(row));
                    });
                    cx.notify();
                }
                TableEvent::ClearSelection => {
                    this.selected_row = None;
                    state.update(cx, |table, _| {
                        table.delegate_mut().sync_selected_row(None);
                    });
                    cx.notify();
                }
                TableEvent::RightClickedRow(Some(row)) => {
                    // Right-clicking selects the row, so the context menu acts
                    // on it (Qt `table_view->setCurrentIndex(index)`).
                    let row = *row;
                    state.update(cx, |table, cx| table.set_selected_row(row, cx));
                }
                TableEvent::ColumnWidthsChanged(widths) => {
                    let widths = widths.clone();
                    let viewport_width = window.viewport_size().width;
                    state.update(cx, |table, cx| {
                        // 1 px delta guard so a sub-pixel change cannot drive a
                        // refresh loop (P8).
                        let changed = {
                            let delegate = table.delegate_mut();
                            if let Some(width) = widths.first() {
                                delegate.name_width = *width;
                            }
                            if let Some(width) = widths.get(1) {
                                delegate.app_id_width = *width;
                            }
                            let compat = (viewport_width
                                - delegate.name_width
                                - delegate.app_id_width
                                - px(24.))
                            .max(px(80.));
                            if (delegate.compat_width - compat).abs() > px(1.) {
                                delegate.compat_width = compat;
                                true
                            } else {
                                false
                            }
                        };
                        if changed {
                            table.refresh(cx);
                        }
                    });
                }
                _ => {}
            },
        );

        let activation_subscription = cx.observe_window_activation(window, |this, _window, cx| {
            // Row highlight is only painted while the window is active (P28),
            // so repaint when activation returns.
            this.table.update(cx, |table, cx| table.refresh(cx));
        });

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let (launch_tx, launch_rx) = async_channel::bounded(LAUNCH_EVENT_CAPACITY);

        let mut app = Self {
            table,
            log,
            log_text: String::new(),
            loading: false,
            selected_row: None,
            status_text: "Ready".to_string(),
            remember_last_directory: crate::config::load_config().remember_last_dir,
            launch_running: false,
            active_launches: 0,
            launch_generation: 0,
            browsing: false,
            viewport_width: None,
            focus_handle,
            repaint_queued: Rc::new(Cell::new(false)),
            _subscriptions: vec![subscription],
            _activation_subscription: activation_subscription,
            _refresh_task: None,
            _launch_events_task: None,
            _browse_task: None,
            _delete_cache_task: None,
            deleting_shader_cache: false,
            launch_tx,
            launch_rx,
        };

        // One long-lived task applies launch events to the UI. The channel
        // receiver is cloned into the task; the root keeps the original plus
        // the sender so the channel never closes.
        app._launch_events_task = Some(cx.spawn_in(window, {
            let launch_rx = app.launch_rx.clone();
            async move |this, cx| {
                while let Ok(event) = launch_rx.recv().await {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.handle_launch_event(event, window, cx);
                    });
                }
            }
        }));

        // Qt starts discovery right after the window is shown. Tests construct
        // the view directly and must stay deterministic (the scan reads the
        // real filesystem), so the automatic first load is production-only.
        #[cfg(not(test))]
        app.refresh_games(window, cx);

        app
    }

    /// Append a timestamped line to both the shadow text and the textarea.
    pub fn append_log(
        &mut self,
        message: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        // Terminate every line, including the last: the trailing empty row
        // gives the no-wrap pane's horizontal scrollbar a blank strip to
        // overlay instead of the newest line's descenders.
        let line = format!("[{}] {}\n", log::timestamp(), message);

        let prior_lines = self.log_text.lines().count();
        let at_bottom = self
            .log
            .read(cx)
            .visible_row_range()
            .is_none_or(|range| range.end >= prior_lines);
        let prior_scroll = self.log.read(cx).scroll_offset();
        let end = self.log_text.len();
        self.log_text.push_str(&line);

        self.log.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            state.insert(line, window, cx);
            // Park the caret at the start of the appended line so a no-wrap
            // pane keeps its left edge instead of chasing a long line's tail
            // sideways.
            state.set_selected_range(end..end, cx);
            // Appending moves the cursor, which keeps the newest line in view.
            // When the user has scrolled up, restore the old offset instead.
            if !at_bottom {
                state.set_scroll_offset(prior_scroll, cx);
            }
            cx.notify();
        });

        let lines = self.log_text.lines().count();
        if lines > MAX_LOG_LINES + TRIM_SLACK {
            let keep_from = lines - MAX_LOG_LINES;
            let tail = format!(
                "{}\n",
                self.log_text
                    .lines()
                    .skip(keep_from)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            self.log_text = tail.clone();
            self.log.update(cx, |state, cx| {
                state.set_value(tail, window, cx);
                // `set_value` resets the viewport to the start; re-apply an
                // explicit offset so the pane keeps following (or keeps the
                // user's position).
                let offset =
                    trim_scroll_offset(at_bottom, prior_scroll, state.line_height(), keep_from);
                state.set_scroll_offset(offset, cx);
            });
        }

        cx.notify();
        // Appends can arrive from background launch tasks; schedule the frame
        // explicitly (see `refresh_games` completion).
        schedule_frame(window, &self.repaint_queued);
    }

    /// Re-run Steam discovery, preserving the table's current sort order.
    ///
    /// While a scan is in flight `loading` is set and further calls are
    /// ignored (the Refresh button is disabled and F5 no-ops), so a completion
    /// always belongs to the current load.
    pub(crate) fn refresh_games(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }

        self.loading = true;
        self.status_text = "Loading games...".to_string();
        cx.notify();
        self.append_log("Refreshing games...", window, cx);

        // Surface the sandbox mode: inside a Flatpak, launches are routed
        // through `flatpak-spawn --host`.
        if crate::flatpak::running_in_flatpak() {
            self.append_log("Flatpak environment detected", window, cx);
        }

        self._refresh_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Discovery is an unbounded filesystem scan; run it on a
            // background task and await the result here (Qt ran the same scan
            // on a worker thread).
            let result = cx
                .background_executor()
                .spawn(async move { crate::steam::discover_games() })
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.loading = false;

                // Resolve the sort state at completion time, not before the
                // scan: a header click during the scan must win (L2), and
                // `ColumnSort::Default` must leave discovery order intact (L1).
                let (sort_column, sort_sort) = {
                    let delegate = this.table.read(cx).delegate();
                    (delegate.sort_column, delegate.sort_sort)
                };

                let (rows, status, warnings) = discovery_outcome(result, sort_column, sort_sort);
                for warning in &warnings {
                    this.append_log(warning.clone(), window, cx);
                }

                // Clear the selection before/with the row replacement, so the
                // action buttons and the status bar never show a stale row.
                this.selected_row = None;
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().set_rows(rows);
                    table.refresh(cx);
                    table.clear_selection(cx);
                });
                this.status_text = status;
                cx.notify();
                schedule_frame(window, &this.repaint_queued);
            });
        }));
    }

    fn on_refresh_games(&mut self, _: &RefreshGames, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_games(window, cx);
    }

    fn on_log_copy(&mut self, _: &LogCopy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.log.read(cx).selected_value();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        }
    }

    fn on_log_select_all(&mut self, _: &LogSelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.log.update(cx, |state, cx| {
            state.select_all(window, cx);
            // The editor only paints its selection while it is focused.
            let handle = state.focus_handle(cx);
            handle.focus(window, cx);
        });
    }

    fn on_log_clear(&mut self, _: &LogClear, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_logs(window, cx);
    }

    fn clear_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.log_text.clear();
        self.log
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        settings_ui::open_settings_dialog(
            self.remember_last_directory,
            cx.weak_entity(),
            window,
            cx,
        );
    }

    fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        about_ui::open_about_dialog(window, cx);
    }

    /// Persist the settings checkbox immediately (the legacy dialog did), and
    /// log the change. Write failures are diagnostics only, as in Qt.
    pub(crate) fn apply_remember_last_directory(
        &mut self,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_last_directory = value;
        self.append_log(
            format!("Settings: remember last directory = {value}"),
            window,
            cx,
        );

        let config = crate::config::AppConfig {
            remember_last_dir: value,
        };
        if let Err(e) = crate::config::save_config(&config) {
            eprintln!("protonctx: failed to save config: {e}");
        }
    }

    /// The currently selected row, cloned out of the table delegate.
    fn selected_row_data(&self, cx: &App) -> Option<GameRow> {
        let row_ix = self.selected_row?;
        self.table.read(cx).delegate().rows.get(row_ix).cloned()
    }

    /// The currently selected row as a backend [`Game`]. The launcher only
    /// reads `name`, `app_id`, `library_path`, and `proton_dir`.
    fn selected_game(&self, cx: &App) -> Option<Game> {
        let row = self.selected_row_data(cx)?;
        Some(Game {
            name: row.name.to_string(),
            app_id: row.app_id,
            compat_tool: row.compat_tool.to_string(),
            library_path: row.library_path,
            proton_dir: row.proton_dir,
        })
    }

    /// Launch a built-in Wine tool in the selected game's prefix.
    pub(crate) fn launch_tool(&mut self, tool: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game(cx) else {
            self.show_launch_error("No game selected", window, cx);
            return;
        };

        match crate::launcher::launch_tool(&game, tool) {
            Ok(proc) => self.start_launch(proc, format!("Running {tool}..."), window, cx),
            Err(e) => {
                self.show_launch_error(format!("Failed to launch {tool}: {e}"), window, cx);
            }
        }
    }

    /// Pick a Windows executable through the XDG desktop portal and run it in
    /// the selected game's prefix.
    pub(crate) fn browse_for_executable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The portal dialog is not application-modal, so the button/context
        // menu can be activated again while the first request is pending.
        // Replacing `_browse_task` would drop (and cancel) the in-flight
        // request, so only one browse may be active at a time.
        if self.browsing {
            return;
        }
        self.browsing = true;

        // Seed the dialog at the last directory when the preference is on;
        // otherwise start at the portal default. The selection is re-resolved
        // after the dialog closes, as Qt does.
        let initial_dir = if self.remember_last_directory {
            crate::config::load_last_dir()
        } else {
            None
        };

        self._browse_task = Some(cx.spawn_in(window, async move |this, cx| {
            let picked = prompt_for_exe(initial_dir).await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Every completion path (picked, cancelled, failed) frees the
                // next browse.
                this.browsing = false;

                match picked {
                    Ok(Some(path)) => {
                        // Remember the *directory* (not the file) for the next open.
                        if this.remember_last_directory
                            && let Some(parent) = path.parent()
                            && let Err(e) = crate::config::save_last_dir(parent)
                        {
                            eprintln!("protonctx: failed to save last directory: {e}");
                        }
                        this.launch_browsed(path, window, cx);
                    }
                    // Portal cancel: no selection is not an error.
                    Err(ashpd::Error::Response(_)) | Ok(None) => {}
                    Err(e) => {
                        this.show_launch_error(
                            format!("Failed to open file dialog: {e}"),
                            window,
                            cx,
                        );
                    }
                }
                // This completion runs outside the window dispatch, so ask
                // for the frame explicitly.
                schedule_frame(window, &this.repaint_queued);
            });
        }));
    }

    fn launch_browsed(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game(cx) else {
            self.show_launch_error("No game selected", window, cx);
            return;
        };

        let display = path.to_string_lossy().into_owned();
        match crate::launcher::proton::run_in_prefix(&game, &[&display]) {
            Ok(proc) => self.start_launch(proc, format!("Running {display}..."), window, cx),
            Err(e) => {
                self.show_launch_error(format!("Failed to launch: {e}"), window, cx);
            }
        }
    }

    /// Mark a launch as running, log its command line, and start the pipe
    /// readers and exit watcher. Mirrors the legacy `start_launch_watcher`.
    fn start_launch(
        &mut self,
        mut proc: crate::launcher::LaunchedProcess,
        status: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.launch_generation = self.launch_generation.wrapping_add(1);
        self.active_launches = self.active_launches.saturating_add(1);
        self.launch_running = true;
        self.status_text = status;

        self.append_log(format!("Starting {}", proc.command_line), window, cx);

        let generation = self.launch_generation;
        let stdout = proc.child.stdout.take();
        let stderr = proc.child.stderr.take();
        let mut child = proc.child;

        // Stream each captured pipe from its own thread (as Qt's `log_pipe`).
        stream_pipe(stdout, self.launch_tx.clone());
        stream_pipe(stderr, self.launch_tx.clone());

        let tx = self.launch_tx.clone();
        let watcher_generation = generation;
        let spawn_result = std::thread::Builder::new()
            .name("launch-watcher".to_string())
            .spawn(move || {
                let result = child.wait();
                let _ = tx.send_blocking(LaunchEvent::Exited { generation, result });
            });
        if let Err(e) = spawn_result {
            // The child is already running, but without the watcher it can
            // never be reaped and `active_launches` would never decrement.
            // Clear the launch state so `launch_running` cannot wedge.
            self.handle_watcher_spawn_failure(e, watcher_generation, window, cx);
        }

        cx.notify();
    }

    /// Clear the launch bookkeeping when the exit-watcher thread could not be
    /// spawned, and surface the failure in the log and status bar.
    fn handle_watcher_spawn_failure(
        &mut self,
        error: std::io::Error,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_launches = self.active_launches.saturating_sub(1);
        self.launch_running = self.active_launches > 0;

        let message = format!("Launch error: failed to start watcher: {error}");
        self.append_log(message.clone(), window, cx);
        // A newer launch superseded this one for status-text purposes; do not
        // clobber its message.
        if generation == self.launch_generation {
            self.status_text = message;
        }
        cx.notify();
    }

    fn handle_launch_event(
        &mut self,
        event: LaunchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            LaunchEvent::Line { text } => {
                self.append_log(text, window, cx);
            }
            LaunchEvent::Exited { generation, result } => {
                self.active_launches = self.active_launches.saturating_sub(1);
                // A newer launch superseded this one for *status text*
                // purposes; do not clobber its message. Every process still
                // logs its own exit event.
                let is_latest = generation == self.launch_generation;
                let still_running = self.active_launches > 0;

                let finished = match result {
                    Ok(status) => match status.code() {
                        Some(0) => "Exit code: 0".to_string(),
                        Some(code) => format!("Exit code: {code}"),
                        None => "Process terminated by signal".to_string(),
                    },
                    Err(e) => format!("Launch error: {e}"),
                };
                self.append_log(finished.clone(), window, cx);

                if is_latest {
                    self.status_text = finished;
                }
                self.launch_running = still_running;
                cx.notify();
            }
        }
    }

    /// Show a message box carrying the actual failure text.
    pub(crate) fn show_launch_error(
        &mut self,
        message: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        // The dialog is transient; keep the failure in the log viewer so the
        // history survives after it is dismissed.
        self.append_log(format!("Launch error: {message}"), window, cx);
        let _ = dialog_ui::open_dialog(window, cx, LaunchErrorContent { message });
    }

    /// Copy the selected game's compatdata (prefix) path, re-resolving the
    /// selection at click time (Qt did the same after async resets).
    pub(crate) fn copy_compat_data_path(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row_data(cx) else {
            return;
        };
        let path = crate::launcher::proton::compat_data_dir_for(
            std::path::Path::new(&row.library_path),
            row.app_id,
        );
        let path = path.to_string_lossy().into_owned();
        if !path.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(path));
        }
    }

    /// Copy the selected game's resolved compatibility-tool path.
    pub(crate) fn copy_compatibility_tool_path(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row_data(cx) else {
            return;
        };
        if !row.proton_dir.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(row.proton_dir));
        }
    }

    /// Confirm then delete the selected game's shader cache. The message text
    /// matches the legacy confirmation dialog.
    pub(crate) fn confirm_delete_shader_cache(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.selected_row_data(cx) else {
            return;
        };
        let path = crate::steam::shadercache::shader_cache_dir_for(
            std::path::Path::new(&row.library_path),
            row.app_id,
        );
        let message = format!(
            "This will permanently delete the shader cache directory:\n\n{}\n\n\
             Shader caches are rebuilt automatically the next time the game runs, \
             but for best results close the game before deleting.",
            path.display()
        );

        // Abort before changing any state when the confirm window cannot open.
        let content = DeleteCacheContent {
            message,
            app: cx.weak_entity(),
            main: window.window_handle(),
            deleting: false,
        };
        let _ = dialog_ui::open_dialog(window, cx, content);
    }

    /// Delete the selected game's shader cache and report the outcome exactly
    /// as the legacy model did. The recursive delete runs on a background task;
    /// the confirm window stays open (non-dismissible) until every outcome is
    /// known, then closes for all three of `Ok(true)`, `Ok(false)`, and `Err`.
    pub(crate) fn commit_delete_shader_cache(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Single-flight: repeat clicks while a deletion is in flight are no-ops
        // (the button is disabled, but a queued click can still land).
        if self.deleting_shader_cache {
            return;
        }
        let Some(row) = self.selected_row_data(cx) else {
            self.show_launch_error("No game selected", window, cx);
            return;
        };
        self.deleting_shader_cache = true;
        // Repaint the confirm window with its buttons disabled and Esc vetoed
        // before yielding.
        let _ = dialog_ui::update_dialog_content::<DeleteCacheContent>(
            "delete-shader-cache",
            cx,
            |content| content.deleting = true,
        );

        self._delete_cache_task = Some(cx.spawn_in(window, async move |this, cx| {
            let library = PathBuf::from(&row.library_path);
            let app_id = row.app_id;
            let result = cx
                .background_executor()
                .spawn(
                    async move { crate::steam::shadercache::delete_shader_cache(&library, app_id) },
                )
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.deleting_shader_cache = false;
                match result {
                    Ok(true) => {
                        this.append_log(
                            format!("Deleted shader cache for {} (app id {})", row.name, app_id),
                            window,
                            cx,
                        );
                        this.status_text = "Shader cache deleted".to_string();
                        dialog_ui::close_dialog("delete-shader-cache", cx);
                    }
                    Ok(false) => {
                        this.append_log(
                            format!("No shader cache found for {} (app id {})", row.name, app_id),
                            window,
                            cx,
                        );
                        this.status_text = "No shader cache to delete".to_string();
                        dialog_ui::close_dialog("delete-shader-cache", cx);
                    }
                    Err(e) => {
                        // Close the confirm first, then surface the error in its
                        // own Launch Error window.
                        dialog_ui::close_dialog("delete-shader-cache", cx);
                        this.show_launch_error(
                            format!("Failed to delete shader cache: {e}"),
                            window,
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        }));
    }

    fn sync_compat_width(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let width = window.viewport_size().width;
        // Viewport guard: bail out when nothing changed, or the refresh below
        // re-enters through the layout and spins (P8).
        if self.viewport_width == Some(width) {
            return;
        }
        self.viewport_width = Some(width);

        let delegate = self.table.read(cx).delegate();
        let compat = (width - delegate.name_width - delegate.app_id_width - px(24.)).max(px(80.));

        self.table.update(cx, |state, cx| {
            state.delegate_mut().compat_width = compat;
            state.refresh(cx);
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();

        h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            // Kirigami draws a ToolBar with the Header colour set
            // (`desktop/ToolBar.qml`), while popup menus use Window; the
            // connector exposes the KDE Header background as `list_head`.
            .bg(cx.theme().list_head)
            .child(
                Button::new("main-menu")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(cx.theme().transparent)
                            .foreground(cx.theme().primary_foreground)
                            .hover(cx.theme().primary)
                            .active(cx.theme().primary),
                    )
                    .icon(IconName::Menu)
                    .dropdown_menu(move |menu, window, _cx| {
                        let Some(entity) = weak.upgrade() else {
                            return menu;
                        };

                        menu.item(menu::item("Settings").icon(IconName::Settings).on_click(
                            window.listener_for(&entity, |app, _, window, cx| {
                                app.open_settings(window, cx);
                            }),
                        ))
                        .item(menu::item("Clear logs").icon(IconName::Eraser).on_click(
                            window.listener_for(&entity, |app, _, window, cx| {
                                app.clear_logs(window, cx);
                            }),
                        ))
                        .item(menu::item("About protonctx").icon(IconName::Info).on_click(
                            window.listener_for(&entity, |app, _, window, cx| {
                                app.open_about(window, cx);
                            }),
                        ))
                        .separator()
                        .item(
                            menu::item("Exit")
                                .icon(IconName::LogOut)
                                .on_click(|_, window, _cx| {
                                    window.remove_window();
                                }),
                        )
                    }),
            )
            .child(
                Button::new("refresh-games")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(cx.theme().transparent)
                            .foreground(cx.theme().primary_foreground)
                            .hover(cx.theme().primary)
                            .active(cx.theme().primary),
                    )
                    .icon(IconName::RefreshCw)
                    .label("Refresh games")
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.refresh_games(window, cx);
                    })),
            )
    }

    fn render_action_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.selected_row.is_some();

        h_flex()
            .gap_1()
            .child(self.action_button("browse", "Browse...", None, enabled, cx))
            .child(self.action_button(
                TOOL_BUTTONS[0].0,
                TOOL_BUTTONS[0].1,
                Some(TOOL_BUTTONS[0].2),
                enabled,
                cx,
            ))
            .child(self.action_button(
                TOOL_BUTTONS[1].0,
                TOOL_BUTTONS[1].1,
                Some(TOOL_BUTTONS[1].2),
                enabled,
                cx,
            ))
            .child(self.action_button(
                TOOL_BUTTONS[2].0,
                TOOL_BUTTONS[2].1,
                Some(TOOL_BUTTONS[2].2),
                enabled,
                cx,
            ))
            .child(self.action_button(
                TOOL_BUTTONS[3].0,
                TOOL_BUTTONS[3].1,
                Some(TOOL_BUTTONS[3].2),
                enabled,
                cx,
            ))
    }

    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        tool: Option<&'static str>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // The styled Button overwrites any caller hover style with its variant
        // hover (`gpui-component/src/button/button.rs`), so an outline border
        // hover color only survives while the button is disabled. Use the
        // unstyled base Button and apply the outline recipe itself.
        let input = cx.theme().input;
        let primary = cx.theme().primary;
        let ring = cx.theme().ring;
        let radius = cx.theme().radius;
        let input_bg = cx.theme().input_background();
        // Qt/Breeze paints the pressed background with the same wash the
        // context menu uses on hover (`MenuItemElement` → `theme.tokens.accent`).
        let pressed_bg = cx.theme().tokens.accent.background;
        let fg = cx.theme().button_foreground;
        let disabled_fg = cx.theme().muted_foreground.opacity(0.5);

        BaseButton::new(id)
            .h(px(32.))
            .px(px(14.))
            .rounded(radius)
            .border_1()
            .text_base()
            .when(enabled, |this| {
                this.border_color(input)
                    .bg(input_bg)
                    .text_color(fg)
                    .hover(move |style| style.border_color(primary))
                    .active(move |style| style.bg(pressed_bg).border_color(primary))
                    .focus_visible(move |style| style.border_color(ring))
            })
            .when(!enabled, |this| {
                this.border_color(input.opacity(0.5))
                    .bg(input_bg.opacity(0.5))
                    .text_color(disabled_fg)
            })
            .disabled(!enabled)
            .accessibility_label(label)
            .on_click(cx.listener(move |this, _, window, cx| match tool {
                Some(tool) => this.launch_tool(tool, window, cx),
                None => this.browse_for_executable(window, cx),
            }))
            .child(label)
    }

    fn render_table_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_1()
            .pb_1()
            .child(
                div().flex_1().min_h_0().child(
                    DataTable::new(&self.table)
                        .bordered(true)
                        .with_size(px(28.))
                        .scrollbar_visible(true, true),
                ),
            )
            .child(self.render_action_buttons(cx))
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let delegate = self.table.read(cx).delegate();
        let right = self
            .selected_row
            .and_then(|row| delegate.rows.get(row))
            .map(|row| format!("Selected: {}", row.app_id))
            .unwrap_or_default();

        StatusBar::new()
            .left(
                h_flex()
                    .items_center()
                    .gap_2()
                    // The Qt status bar showed an indeterminate progress bar
                    // while a launch was in flight.
                    .when(self.launch_running, |this| {
                        this.child(Spinner::new().small())
                    })
                    .child(self.status_text.clone()),
            )
            .right(right)
            .text_size(cx.theme().font_size)
            .text_color(cx.theme().foreground)
    }

    fn render_split(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_resizable("main-split")
            .with_handle_appearance(Rc::new(|_, _, _| Some(div().into_any_element())))
            .child(
                resizable_panel()
                    .size(px(420.))
                    .size_range(px(160.)..Pixels::MAX)
                    .child(self.render_table_pane(cx)),
            )
            .child(
                resizable_panel()
                    .size(px(140.))
                    .size_range(px(54.)..Pixels::MAX)
                    .child(logs_ui::render_log_pane(&self.log, cx)),
            )
    }
}

/// Body of the native Launch Error window, laid out like the Qt/Breeze message
/// box: a large danger badge with the message text top-aligned beside it.
struct LaunchErrorContent {
    message: String,
}

impl dialog_ui::DialogContent for LaunchErrorContent {
    fn id(&self) -> &'static str {
        "launch-error"
    }

    fn title(&self) -> SharedString {
        "Launch Error".into()
    }

    /// Wide and short, matching the Qt message box proportions.
    fn size(&self) -> Size<Pixels> {
        size(px(500.), px(130.))
    }

    fn body(&mut self, _window: &mut Window, cx: &mut App) -> AnyElement {
        h_flex()
            .w_full()
            .items_start()
            .gap_5()
            .child(error_badge(cx))
            .child(
                v_flex()
                    .id("launch-error-message")
                    .test_support()
                    .debug_selector(|| "launch-error-message".into())
                    .flex_1()
                    .min_w_0()
                    .child(dialog_ui::dialog_text(self.message.clone())),
            )
            .into_any_element()
    }

    fn actions(&self, _cx: &App) -> Vec<dialog_ui::DialogAction> {
        vec![
            dialog_ui::DialogAction::new("launch-error-close", "OK")
                .icon(IconName::Check)
                .default_button(),
        ]
    }
}

/// Filled danger circle with a white cross: the Qt message-box error icon,
/// drawn from theme colors plus the lucide `X` glyph.
fn error_badge(cx: &App) -> impl IntoElement {
    div()
        .id("launch-error-icon")
        .test_support()
        .debug_selector(|| "launch-error-icon".into())
        .flex_shrink_0()
        .size(px(56.))
        .rounded_full()
        .bg(cx.theme().danger)
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(IconName::X).size(px(32.)).text_color(white()))
}

/// Body of the native Delete Shader Cache confirmation. Dismissal (Esc, WM
/// close, Cancel) is vetoed while the deletion is in flight.
///
/// `deleting` is a snapshot because the first frame renders while the owning
/// entity is being updated; `commit_delete_shader_cache` pushes the flip with
/// [`dialog_ui::update_dialog_content`].
struct DeleteCacheContent {
    message: String,
    app: WeakEntity<ProtonctxApp>,
    main: AnyWindowHandle,
    deleting: bool,
}

impl dialog_ui::DialogContent for DeleteCacheContent {
    fn id(&self) -> &'static str {
        "delete-shader-cache"
    }

    fn title(&self) -> SharedString {
        "Delete Shader Cache".into()
    }

    /// Qt's `QMessageBox::warning` proportions: wide enough for the path and
    /// the closing paragraph, with the shorter height of the Qt dialog.
    fn size(&self) -> Size<Pixels> {
        size(px(520.), px(230.))
    }

    fn body(&mut self, _window: &mut Window, cx: &mut App) -> AnyElement {
        div()
            .id("delete-cache-message")
            .test_support()
            .debug_selector(|| "delete-cache-message".into())
            .child(dialog_ui::dialog_warning_body(self.message.clone(), cx))
            .into_any_element()
    }

    fn actions(&self, _cx: &App) -> Vec<dialog_ui::DialogAction> {
        let deleting = self.deleting;
        let app = self.app.clone();
        let main = self.main;
        vec![
            dialog_ui::DialogAction::new("delete-cache-confirm", "Yes")
                .icon(IconName::Check)
                .enabled(!deleting)
                .on_click(move |_, _, cx| {
                    let _ = dialog_ui::with_window_and_entity(main, &app, cx, |app, window, cx| {
                        app.commit_delete_shader_cache(window, cx);
                    });
                }),
            // Qt shows No to the right and makes it the default (safe) button.
            dialog_ui::DialogAction::new("delete-cache-cancel", "No")
                .icon(IconName::Ban)
                .default_button()
                .enabled(!deleting),
        ]
    }

    fn dismissible(&self, _cx: &App) -> bool {
        !self.deleting
    }
}

impl Render for ProtonctxApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_compat_width(window, cx);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_refresh_games))
            .on_action(cx.listener(Self::on_log_copy))
            .on_action(cx.listener(Self::on_log_select_all))
            .on_action(cx.listener(Self::on_log_clear))
            .child(self.render_toolbar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .pb_1()
                    .child(self.render_split(cx)),
            )
            .child(self.render_status_bar(cx))
    }
}

/// Schedule a repaint after an async completion.
///
/// On Linux, the window invalidator has no platform waker, so `cx.notify()`
/// alone marks the view dirty without waking the platform's frame source.
/// `on_next_frame` wakes that source, and its callback marks the window dirty
/// before the requested frame is drawn.
///
/// Scheduling is single-flight: while a callback is pending, further requests
/// are coalesced into it. A stalled frame loop (occluded window, no platform
/// waker) therefore holds at most one callback instead of one per log line.
fn schedule_frame(window: &mut Window, repaint_queued: &Rc<Cell<bool>>) {
    if repaint_queued.replace(true) {
        return;
    }

    let repaint_queued = repaint_queued.clone();
    window.on_next_frame(move |window, _cx| {
        repaint_queued.set(false);
        window.refresh();
    });
}

/// Scroll offset to apply after a log trim.
///
/// `set_value` resets the viewport to the start, so the caller re-applies an
/// explicit offset: a following pane is sent to the tail (the [`f32::MAX`]
/// sentinel is clamped to the maximum on paint), and a scrolled-up pane keeps
/// its position, shifted up by the removed lines and clamped at the top. With
/// no layout yet there is no line height, so the prior offset is kept.
fn trim_scroll_offset(
    at_bottom: bool,
    prior_scroll: Point<Pixels>,
    line_height: Option<Pixels>,
    removed_lines: usize,
) -> Point<Pixels> {
    if at_bottom {
        point(px(0.), px(f32::MAX))
    } else {
        let removed = line_height.map_or(px(0.), |line_height| line_height * removed_lines as f32);
        point(prior_scroll.x, (prior_scroll.y + removed).min(px(0.)))
    }
}

/// Stream one captured pipe (stdout or stderr) of a launched child to the log.
///
/// Runs on its own thread: reads `reader` line-by-line (blocking until the
/// child closes the pipe) and forwards each line through `tx`.
fn stream_pipe<R: std::io::Read + Send + 'static>(
    reader: Option<R>,
    tx: async_channel::Sender<LaunchEvent>,
) {
    let Some(reader) = reader else {
        return;
    };

    let _ = std::thread::Builder::new()
        .name("launch-output".to_string())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(reader).lines() {
                let text = match line {
                    Ok(text) => text,
                    Err(e) => {
                        let _ = tx.send_blocking(LaunchEvent::Line {
                            text: format!("read error: {e}"),
                        });
                        break;
                    }
                };
                let _ = tx.send_blocking(LaunchEvent::Line { text });
            }
        });
}

/// Prompt for a Windows executable through the XDG desktop portal.
///
/// `Ok(None)` means the response carried no usable local path; a cancelled
/// dialog surfaces as `Err(ashpd::Error::Response(_))` and is handled by the
/// caller as a no-op.
async fn prompt_for_exe(initial_dir: Option<PathBuf>) -> ashpd::Result<Option<PathBuf>> {
    use ashpd::desktop::file_chooser::{FileFilter, OpenFileRequest};

    let request = OpenFileRequest::default()
        .title("Select executable to run")
        .multiple(false)
        .current_folder::<PathBuf>(initial_dir)?
        .filter(
            FileFilter::new("Windows executables")
                .glob("*.exe")
                .glob("*.EXE")
                .glob("*.bat")
                .glob("*.cmd"),
        )
        .filter(FileFilter::new("All Files").glob("*"))
        .send()
        .await?;
    let response = request.response()?;

    Ok(response
        .uris()
        .iter()
        .filter_map(|uri| url::Url::parse(uri.as_str()).ok())
        .find_map(|uri| uri.to_file_path().ok()))
}

/// Map a discovery result to rows, status text, and log warnings, sorting the
/// rows with the caller's current sort state (Qt's `load_games` contract).
fn discovery_outcome(
    result: Result<crate::steam::Discovery, crate::steam::SteamError>,
    sort_column: usize,
    sort_sort: ColumnSort,
) -> (Vec<GameRow>, String, Vec<String>) {
    match result {
        Ok(discovery) => {
            let mut rows: Vec<GameRow> = discovery.games.iter().map(GameRow::from_game).collect();
            match sort_sort {
                ColumnSort::Ascending => sort_games(&mut rows, sort_column, true),
                ColumnSort::Descending => sort_games(&mut rows, sort_column, false),
                // The neutral header state means "discovery order".
                ColumnSort::Default => {}
            }

            let count = rows.len();
            let status = match count {
                0 => "No games found".to_string(),
                1 => "1 game loaded".to_string(),
                n => format!("{n} games loaded"),
            };
            (rows, status, discovery.warnings)
        }
        Err(crate::steam::SteamError::SteamNotFound) => {
            (Vec::new(), "Steam not found".to_string(), Vec::new())
        }
        Err(e) => {
            let msg = format!("Failed to discover games: {e}");
            (Vec::new(), msg.clone(), vec![msg])
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{
        Root,
        table::{ColumnSort, TableDelegate as _},
    };
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Focusable as _, Keystroke, TestAppContext, point, px,
        size,
    };

    use super::{
        ProtonctxApp, TRIM_SLACK, bind_keys, discovery_outcome, schedule_frame, trim_scroll_offset,
    };
    use crate::games::GameRow;
    use crate::models::Game;
    use crate::views::dialog_ui;

    /// The open native dialog window registered under `id`.
    fn dialog_window(id: &str, cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(|cx| dialog_ui::window_for(id, cx))
            .unwrap_or_else(|| panic!("no open {id} dialog window"))
    }

    fn test_game(name: &str, app_id: u32) -> Game {
        Game {
            name: name.to_string(),
            app_id,
            compat_tool: "proton_experimental".to_string(),
            library_path: "/lib".to_string(),
            proton_dir: "/lib/steamapps/common/Proton - Experimental".to_string(),
        }
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
        let (rows, status, warnings) = discovery_outcome(Ok(discovery), 1, ColumnSort::Descending);
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![274190, 204360]
        );
        assert_eq!(status, "2 games loaded");
        assert_eq!(warnings, vec!["a warning".to_string()]);

        let (rows, status, warnings) = discovery_outcome(
            Err(crate::steam::SteamError::SteamNotFound),
            0,
            ColumnSort::Ascending,
        );
        assert!(rows.is_empty());
        assert_eq!(status, "Steam not found");
        assert!(warnings.is_empty());

        let error = crate::steam::SteamError::Parse("bad vdf".to_string());
        let (rows, status, warnings) = discovery_outcome(Err(error), 0, ColumnSort::Ascending);
        assert!(rows.is_empty());
        assert_eq!(status, "Failed to discover games: parse error: bad vdf");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn discovery_outcome_leaves_rows_unsorted_for_default_sort() {
        // `ColumnSort::Default` is the neutral header state: a refresh must
        // preserve discovery order instead of implying a descending sort.
        let discovery = crate::steam::Discovery {
            games: vec![
                test_game("Castle Crashers", 204360),
                test_game("Broforce", 274190),
            ],
            warnings: Vec::new(),
        };
        let (rows, status, warnings) = discovery_outcome(Ok(discovery), 0, ColumnSort::Default);
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![204360, 274190]
        );
        assert_eq!(status, "2 games loaded");
        assert!(warnings.is_empty());
    }

    #[gpui_kit::test]
    fn settings_menu_item_opens_dialog_and_ok_closes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let main_id = main.window_id();
        let _view = view.unwrap();

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "Settings opens a native window");
        assert_ne!(dialog_window("settings", cx).window_id(), main_id);

        let settings = dialog_window("settings", cx);
        cx.update_window(settings, |_, window, cx| {
            window.render_frame(cx);
            window.click("settings-ok", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 1, "OK closes the Settings window");
        assert!(cx.update(|cx| !dialog_ui::is_open("settings", cx)));
    }

    #[gpui_kit::test]
    fn about_menu_item_opens_window(cx: &mut TestAppContext) {
        use native_theme_gpui::AccessibilityPreferences;

        cx.update(gpui_kit::init);
        // The description width check is only meaningful at the production KDE
        // 10 pt rem; the bare kit theme leaves the 16 px test default in place.
        cx.update(|cx| {
            crate::system_theme::install_preset(&AccessibilityPreferences::default(), true, cx);
        });

        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            Root::new(app, window, cx)
        });
        let main_id = main.window_id();

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "About opens a second window");
        let about = dialog_window("about", cx);
        assert_ne!(about.window_id(), main_id);

        cx.update_window(about, |_, window, cx| {
            window.render_frame(cx);
            // Fixed px sizing keeps action buttons at Qt height even at the
            // production rem, which is smaller than the test default.
            let ok = window.find("about-ok").bounds();
            assert_eq!(ok.size.height, px(32.), "action button is not Qt-height");
            assert!(
                ok.size.width >= px(80.),
                "action button is narrower than Qt's"
            );
            window.click("about-ok", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "Ok closes the About window");
        assert!(cx.update(|cx| !dialog_ui::is_open("about", cx)));
    }

    #[gpui_kit::test]
    fn about_dialog_focuses_ok_and_enter_closes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            Root::new(app, window, cx)
        });

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            crate::views::about_ui::open_about_dialog(window, cx);
        })
        .unwrap();

        let about = dialog_window("about", cx);
        cx.update_window(about, |_, window, cx| {
            // The first frame lays out the footer and schedules the focus
            // hand-off; the next-frame callback performs it, like Qt focusing
            // its default button on show.
            window.render_frame(cx);
            let _ = window.simulate_next_frame(cx);
            assert!(
                window.focused(cx).is_some(),
                "the default Ok button should hold focus"
            );
            // Enter activates the focused default button.
            window.press("enter", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 1, "Enter activated the default Ok");
        assert!(cx.update(|cx| !dialog_ui::is_open("about", cx)));
    }

    #[gpui_kit::test]
    fn launch_error_dialog_focuses_ok_and_enter_closes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.show_launch_error("Failed to launch winecfg", window, cx);
            });
        })
        .unwrap();

        let error = dialog_window("launch-error", cx);
        cx.update_window(error, |_, window, cx| {
            window.render_frame(cx);
            let _ = window.simulate_next_frame(cx);
            assert!(
                window.focused(cx).is_some(),
                "the default OK button should hold focus"
            );
            window.press("enter", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 1, "Enter activated the default OK");
        assert!(cx.update(|cx| !dialog_ui::is_open("launch-error", cx)));
    }

    #[gpui_kit::test]
    fn about_dialog_dedupes_and_cascades(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(main.into(), |_, window, cx| {
            view.update(cx, |app, cx| app.open_about(window, cx));
            let first = dialog_ui::window_for("about", cx)
                .expect("About opens")
                .window_id();
            view.update(cx, |app, cx| app.open_about(window, cx));
            let second = dialog_ui::window_for("about", cx)
                .expect("About stays open")
                .window_id();
            assert_eq!(first, second, "reopen must reuse the About window");
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "no duplicate About window");

        cx.update_window(main.into(), |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();

        assert!(
            cx.update(|cx| dialog_ui::window_for("about", cx)).is_none(),
            "About outlived its parent"
        );
        assert!(cx.windows().is_empty(), "orphan dialog window remains");
    }

    #[gpui_kit::test]
    fn about_dialog_exposes_a11y_dialog_role(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            Root::new(app, window, cx)
        });

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            crate::views::about_ui::open_about_dialog(window, cx);
        })
        .unwrap();

        // `debug_a11y_tree_json` only fills in once a platform adapter activates
        // accessibility, which the headless platform does not do. The observed
        // element facts expose the same role/label the a11y node is built from.
        let about = dialog_window("about", cx);
        cx.update_window(about, |_, window, cx| {
            window.render_frame(cx);
            let root = window.find("dialog-root");
            assert!(root.visible());
            assert_eq!(root.role(), Some(gpui_kit::Role::Dialog));
            assert_eq!(root.label(), Some("About protonctx"));
        })
        .unwrap();
    }

    /// The settings checkbox persists immediately when toggled. Driven by
    /// [`settings_persist_config_subprocess`] through a child process with
    /// `XDG_CONFIG_HOME` pointed at a temporary directory; running it in the
    /// parent would write the developer's real config, so it skips unless the
    /// child's marker variable is present.
    #[gpui_kit::test]
    fn settings_checkbox_persists_config_to_xdg_config_home(cx: &mut TestAppContext) {
        let Some(config_home) = std::env::var_os("PROTONCTX_TEST_XDG_CONFIG_HOME") else {
            return;
        };

        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "Settings opens a native window");
        let settings = dialog_window("settings", cx);
        cx.update_window(settings, |_, window, cx| {
            window.render_frame(cx);
            // The temp XDG root has no config, so the default (true) is shown;
            // toggling writes `false`.
            window.press("tab", cx);
            window.press("space", cx);
            assert!(!view.read(cx).remember_last_directory);

            window.click("settings-ok", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "OK closes the Settings window");

        let config_path = std::path::Path::new(&config_home)
            .join("protonctx")
            .join("config.json");
        let text = std::fs::read_to_string(&config_path)
            .unwrap_or_else(|e| panic!("{}: {e}", config_path.display()));
        assert!(text.contains("\"remember_last_dir\": false"), "{text}");
    }

    #[test]
    fn settings_persist_config_subprocess() {
        let config_home = std::env::temp_dir().join(format!(
            "protonctx-test-xdg-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&config_home).unwrap();

        let exe = std::env::current_exe().unwrap();
        let status = std::process::Command::new(exe)
            .arg("settings_checkbox_persists_config_to_xdg_config_home")
            .arg("--nocapture")
            .env("XDG_CONFIG_HOME", &config_home)
            .env("PROTONCTX_TEST_XDG_CONFIG_HOME", &config_home)
            .status()
            .unwrap();

        let config_path = config_home.join("protonctx").join("config.json");
        let text = std::fs::read_to_string(&config_path).unwrap_or_default();
        std::fs::remove_dir_all(&config_home).ok();

        assert!(status.success(), "child test process failed");
        assert!(
            text.contains("\"remember_last_dir\": false"),
            "config was not persisted: {text:?}"
        );
    }

    #[gpui_kit::test]
    fn clear_logs_menu_item_clears_the_log(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Seed a line so clearing is observable.
            view.update(cx, |app, cx| {
                app.append_log("seed line", window, cx);
            });
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx); // Settings
            window.press("down", cx); // Clear logs
        })
        .unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.press("enter", cx);
            assert!(view.read(cx).log_text.is_empty());
            let log = view.read(cx).log.clone();
            assert!(log.read(cx).value().is_empty());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn log_clear_keybinding_clears_the_log(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(bind_keys);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.append_log("seed line", window, cx);
                // Focus the read-only log textarea: `ctrl-l` must reach the
                // root action rather than being swallowed by the editor.
                let focus = app.log.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            });
            window.render_frame(cx);
            window.press("ctrl-l", cx);

            assert!(view.read(cx).log_text.is_empty());
            let log = view.read(cx).log.clone();
            assert!(log.read(cx).value().is_empty());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn log_select_all_keybinding_selects_the_log(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(bind_keys);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.append_log("seed line", window, cx);
            });
            // The root is focused, so the global `LogSelectAll` binding (not
            // the textarea-native `ctrl-a`) handles the chord.
            window.dispatch_keystroke(Keystroke::parse("ctrl-a").unwrap(), cx);

            let selected = view.read(cx).log.read(cx).selected_value().to_string();
            assert!(selected.contains("seed line"), "selection was {selected:?}");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn log_copy_keybinding_copies_the_selection(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(bind_keys);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.append_log("seed line", window, cx);
            });
            // Select all through the action, then put focus back on the root so
            // the global `LogCopy` binding (not the textarea-native copy)
            // handles `ctrl-c`.
            window.dispatch_keystroke(Keystroke::parse("ctrl-a").unwrap(), cx);
            let root_focus = view.read(cx).focus_handle.clone();
            root_focus.focus(window, cx);

            window.dispatch_keystroke(Keystroke::parse("ctrl-c").unwrap(), cx);

            let copied = cx.read_from_clipboard().and_then(|item| item.text());
            assert!(
                copied
                    .as_deref()
                    .is_some_and(|text| text.contains("seed line")),
                "clipboard did not receive the selected log text: {copied:?}"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn schedule_frame_is_single_flight(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            // Drain anything queued during window construction.
            let _ = window.simulate_next_frame(cx);

            let queued = std::rc::Rc::new(std::cell::Cell::new(false));
            schedule_frame(window, &queued);
            schedule_frame(window, &queued);
            assert!(queued.get(), "a callback should be pending");
            // Two requests coalesce into exactly one queued callback...
            assert_eq!(window.simulate_next_frame(cx), 1);
            // ...whose delivery clears the flag.
            assert!(!queued.get(), "delivery should clear the flag");

            schedule_frame(window, &queued);
            assert_eq!(window.simulate_next_frame(cx), 1);

            // `append_log` goes through the same helper, so the flag is set and
            // a delivered frame clears it.
            view.update(cx, |app, cx| {
                app.append_log("first", window, cx);
                app.append_log("second", window, cx);
                assert!(app.repaint_queued.get(), "the append did not queue a frame");
            });
            let _ = window.simulate_next_frame(cx);
            assert!(
                !view.read(cx).repaint_queued.get(),
                "a delivered frame should clear the flag"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn watcher_spawn_failure_clears_launch_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.active_launches = 1;
                app.launch_running = true;
                app.launch_generation = 7;

                app.handle_watcher_spawn_failure(
                    std::io::Error::other("resource temporarily unavailable"),
                    7,
                    window,
                    cx,
                );

                assert_eq!(app.active_launches, 0);
                assert!(!app.launch_running);
                assert!(app.status_text.contains("failed to start watcher"));
                assert!(app.log_text.contains("failed to start watcher"));
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn append_log_caps_line_count(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            for line in 0..super::MAX_LOG_LINES + TRIM_SLACK + 20 {
                view.update(cx, |app, cx| {
                    app.append_log(format!("line {line}"), window, cx);
                });
            }

            let lines = view.read(cx).log_text.lines().count();
            assert!(
                (super::MAX_LOG_LINES..=super::MAX_LOG_LINES + TRIM_SLACK).contains(&lines),
                "expected {}..={} lines after trim, got {lines}",
                super::MAX_LOG_LINES,
                super::MAX_LOG_LINES + TRIM_SLACK
            );
            let shadow = view.read(cx).log_text.clone();
            assert!(
                !shadow.contains("] line 0\n"),
                "oldest line was not trimmed"
            );
            assert!(
                shadow.ends_with(&format!(
                    "] line {}\n",
                    super::MAX_LOG_LINES + TRIM_SLACK + 19
                )),
                "newest line was trimmed"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn refresh_games_logs_an_entry(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                // The automatic first load is production-only, so the initial
                // refresh is explicit here; the entry is written before the
                // scan is spawned.
                app.refresh_games(window, cx);
                assert!(
                    app.log_text.contains("Refreshing games..."),
                    "refresh was not logged: {:?}",
                    app.log_text
                );
            });
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn log_pane_scrolls_horizontally_instead_of_wrapping(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            // A single line far wider than the pane.
            view.update(cx, |app, cx| app.append_log("x".repeat(400), window, cx));
            window.render_frame(cx);

            let log = view.read(cx).log.clone();
            log.update(cx, |state, cx| {
                state.set_scroll_offset(point(px(-50.), px(0.)), cx);
            });
            window.render_frame(cx);

            // A no-wrap pane has horizontal overflow, so the offset survives;
            // a wrapping pane has none and clamps it back to zero.
            let offset = view.read(cx).log.read(cx).scroll_offset();
            assert!(
                offset.x < px(0.),
                "log pane did not scroll horizontally: {offset:?}"
            );
        })
        .unwrap();
    }

    #[test]
    fn trim_scroll_offset_follows_or_preserves_the_viewport() {
        // Following: the sentinel is clamped to the tail on paint.
        assert_eq!(
            trim_scroll_offset(true, point(px(0.), px(-100.)), Some(px(20.)), 50),
            point(px(0.), px(f32::MAX))
        );

        // Scrolled up: shift up by the removed lines so the same content
        // stays in view.
        assert_eq!(
            trim_scroll_offset(false, point(px(0.), px(-1020.)), Some(px(20.)), 50),
            point(px(0.), px(-20.))
        );

        // The shift cannot push the view above the retained text.
        assert_eq!(
            trim_scroll_offset(false, point(px(0.), px(-30.)), Some(px(20.)), 50),
            point(px(0.), px(0.))
        );

        // Before the first layout there is no line height; keep the offset.
        assert_eq!(
            trim_scroll_offset(false, point(px(0.), px(-30.)), None, 50),
            point(px(0.), px(-30.))
        );
    }

    #[gpui_kit::test]
    fn launch_error_dialog_wraps_long_messages(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        let long = "Failed to launch /home/user/.local/share/Steam/steamapps/common/\
                    Proton - Experimental/files/bin/wine64 with a very long argument list"
            .repeat(2);
        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| {
                app.show_launch_error(long.clone(), window, cx);
            });
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "Launch Error opens a native window");
        let error = dialog_window("launch-error", cx);
        cx.update_window(error, |_, window, cx| {
            window.render_frame(cx);

            // Qt-like geometry: 56 px danger badge, text column beside it, and
            // a 32 px-high action button at least 80 px wide.
            let icon = window.find("launch-error-icon").bounds();
            assert_eq!(icon.size, size(px(56.), px(56.)), "icon is not Qt-sized");
            let button = window.find("launch-error-close").bounds();
            assert_eq!(button.size.height, px(32.), "button is not Qt-height");
            assert!(
                button.size.width >= px(80.),
                "button is narrower than Qt's: {button:?}"
            );

            let message = window.find("launch-error-message");
            let bounds = message.bounds();
            // A single line at the test rem is 20 px; anything taller means the
            // text wrapped instead of keeping its intrinsic width.
            assert!(
                bounds.size.height > px(20.),
                "long message did not wrap: {bounds:?}"
            );
            // 500 px window minus the 16 px side paddings, the 56 px icon, and
            // the 20 px gap; wider than that would be clipped.
            assert!(
                bounds.size.width <= px(396.),
                "message exceeded the dialog body width: {bounds:?}"
            );
        })
        .unwrap();

        // A second error replaces the message in the same window instead of
        // stacking another one.
        cx.update_window(main.into(), |_, window, cx| {
            view.update(cx, |app, cx| {
                app.show_launch_error("short failure", window, cx);
            });
            let logged = view.read(cx).log_text.clone();
            assert!(
                logged.contains("Launch error: short failure"),
                "launch error was not logged: {logged:?}"
            );
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 2, "repeated error stacked windows");
        let error = dialog_window("launch-error", cx);
        cx.update_window(error, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.find("launch-error-message").bounds().size.height <= px(20.),
                "replacement message did not replace the old text"
            );
            window.click("launch-error-close", cx);
        })
        .unwrap();
        assert_eq!(cx.windows().len(), 1, "Close dismisses the error window");
    }

    #[gpui_kit::test]
    fn delete_shader_cache_dialog_closes_after_deletion(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let root = std::env::temp_dir().join(format!(
            "protonctx-test-delete-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let app_id = 274190;
        let cache = crate::steam::shadercache::shader_cache_dir_for(&root, app_id);
        std::fs::create_dir_all(cache.join("fozpipelinesv6")).unwrap();

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        let mut game = test_game("Cache Test", app_id);
        game.library_path = root.to_string_lossy().into_owned();
        view.update(cx, |app, cx| {
            app.table.update(cx, |table, _| {
                table
                    .delegate_mut()
                    .set_rows(vec![GameRow::from_game(&game)]);
            });
            app.selected_row = Some(0);
        });

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| app.confirm_delete_shader_cache(window, cx));
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 2, "confirm opens a native window");
        let confirm = dialog_window("delete-shader-cache", cx);
        cx.update_window(confirm, |_, window, cx| {
            window.render_frame(cx);
            window.click("delete-cache-confirm", cx);
            assert!(view.read(cx).deleting_shader_cache);

            // In flight the confirm is locked: repeat clicks are inert (the
            // core Button disables pointer activation, and commit is
            // single-flight) and Esc is vetoed by `dismissible`.
            window.render_frame(cx);
            window.click("delete-cache-confirm", cx);
            assert!(view.read(cx).deleting_shader_cache);
            window.press("escape", cx);
        })
        .unwrap();
        assert_eq!(
            cx.windows().len(),
            2,
            "Esc must not close the in-flight confirm"
        );

        // The recursive delete runs on the background executor; drive the tasks
        // until the completion handler has run.
        cx.run_until_parked();

        assert_eq!(cx.windows().len(), 1, "confirm closes after deletion");
        assert!(cx.update(|cx| !dialog_ui::is_open("delete-shader-cache", cx)));
        cx.update(|cx| {
            assert!(!view.read(cx).deleting_shader_cache);
            assert_eq!(view.read(cx).status_text, "Shader cache deleted");
            assert!(
                view.read(cx)
                    .log_text
                    .contains("Deleted shader cache for Cache Test (app id 274190)"),
                "log did not report the deletion: {}",
                view.read(cx).log_text
            );
        });

        assert!(!cache.exists(), "cache directory survived the delete");
        std::fs::remove_dir_all(&root).ok();
    }

    #[gpui_kit::test]
    fn delete_shader_cache_failure_closes_confirm_and_opens_error(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        // A relative library path is rejected by the shadercache guard, so the
        // background deletion returns `Err` without touching the filesystem.
        let mut game = test_game("Relative", 274190);
        game.library_path = "relative-lib".to_string();
        view.update(cx, |app, cx| {
            app.table.update(cx, |table, _| {
                table
                    .delegate_mut()
                    .set_rows(vec![GameRow::from_game(&game)]);
            });
            app.selected_row = Some(0);
        });

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            view.update(cx, |app, cx| app.confirm_delete_shader_cache(window, cx));
        })
        .unwrap();

        let confirm = dialog_window("delete-shader-cache", cx);
        cx.update_window(confirm, |_, window, cx| {
            window.render_frame(cx);
            window.click("delete-cache-confirm", cx);
        })
        .unwrap();

        cx.run_until_parked();

        // The confirm closed first, then the error box opened as its own window.
        assert_eq!(cx.windows().len(), 2, "error window replaces the confirm");
        assert!(cx.update(|cx| !dialog_ui::is_open("delete-shader-cache", cx)));
        let error = dialog_window("launch-error", cx);
        cx.update_window(error, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("launch-error-close").visible());
            assert!(!view.read(cx).deleting_shader_cache);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn delete_shader_cache_is_single_flight(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);

            // Without a selection the guard falls through to the existing
            // "No game selected" error window.
            view.update(cx, |app, cx| app.commit_delete_shader_cache(window, cx));
            assert!(dialog_ui::is_open("launch-error", cx));
            dialog_ui::close_dialog("launch-error", cx);

            // While a deletion is in flight a second call is a no-op and must
            // not spawn another task.
            view.update(cx, |app, cx| {
                app.deleting_shader_cache = true;
                app.commit_delete_shader_cache(window, cx);
                assert!(app._delete_cache_task.is_none());
                assert!(app.deleting_shader_cache);
            });
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 1, "no error window after the no-op");
        assert!(cx.update(|cx| !dialog_ui::is_open("launch-error", cx)));
    }

    #[gpui_kit::test]
    fn app_uses_the_statically_installed_theme(cx: &mut TestAppContext) {
        use gpui_kit::component::{Theme, ThemeMode, scroll::ScrollbarMode, try_parse_color};
        use native_theme_gpui::AccessibilityPreferences;

        cx.update(gpui_kit::init);
        cx.update(|cx| {
            crate::system_theme::install_preset(&AccessibilityPreferences::default(), true, cx);
            crate::theme::init(cx);
        });

        let mut view = None;
        let _handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let _view = view.unwrap();

        // The static startup theme is never replaced by the platform
        // appearance, which on Linux starts as Light until the portal answers.
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert_eq!(theme.mode, ThemeMode::Dark);
            assert_eq!(theme.background, try_parse_color("#202326").unwrap());
            assert_eq!(theme.scrollbar_mode, ScrollbarMode::Always);
        });
    }

    #[gpui_kit::test]
    fn header_click_toggles_two_state_sort(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let main = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        view.update(cx, |app, cx| {
            app.table.update(cx, |table, _| {
                let delegate = table.delegate_mut();
                // Name-ascending discovery order matches the delegate's initial
                // sort state (column 0, ascending), so no neutral start is needed.
                delegate.set_rows(vec![
                    GameRow::from_game(&test_game("Broforce", 274190)),
                    GameRow::from_game(&test_game("Castle Crashers", 204360)),
                ]);
            });
        });

        let assert_sorted =
            |cx: &mut TestAppContext, column: usize, sort: ColumnSort, ids: &[u32]| {
                cx.update(|cx| {
                    let table = view.read(cx).table.read(cx);
                    let delegate = table.delegate();
                    assert_eq!(delegate.sort_column, column);
                    assert_eq!(delegate.sort_sort, sort);
                    let actual: Vec<u32> = delegate.rows.iter().map(|row| row.app_id).collect();
                    assert_eq!(actual, ids);
                });
            };

        // Click 1: active column's header body centre -> Descending.
        cx.update_window(main.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("col-header", 0usize), cx);
        })
        .unwrap();
        assert_sorted(cx, 0, ColumnSort::Descending, &[204360, 274190]);

        // Click 2: indicator area of the active column -> one click, one step.
        cx.update_window(main.into(), |_, window, cx| {
            let header = window.find(("col-header", 0usize));
            let offset = point(header.bounds().size.width - px(10.), px(16.));
            window.click_at(("col-header", 0usize), offset, cx);
        })
        .unwrap();
        assert_sorted(cx, 0, ColumnSort::Ascending, &[274190, 204360]);

        // Click 3: a newly clicked column starts ascending (Qt default).
        cx.update_window(main.into(), |_, window, cx| {
            window.click(("col-header", 1usize), cx);
        })
        .unwrap();
        assert_sorted(cx, 1, ColumnSort::Ascending, &[204360, 274190]);

        // Click 4: further clicks on the active column toggle direction.
        cx.update_window(main.into(), |_, window, cx| {
            window.click(("col-header", 1usize), cx);
        })
        .unwrap();
        assert_sorted(cx, 1, ColumnSort::Descending, &[274190, 204360]);

        // Click 5: returning to a column restarts ascending, not a stale toggle.
        cx.update_window(main.into(), |_, window, cx| {
            window.click(("col-header", 0usize), cx);
        })
        .unwrap();
        assert_sorted(cx, 0, ColumnSort::Ascending, &[274190, 204360]);

        // The app-level sort switch stays on, but no column advertises library
        // sort metadata, so the tri-state icon/cycle stays disabled.
        cx.update(|cx| {
            let table = view.read(cx).table.read(cx);
            assert!(table.sortable, "the app-level sort switch stays on");
            for col_ix in 0..3 {
                assert!(
                    table.delegate().column(col_ix, cx).sort.is_none(),
                    "no library sort metadata, so the tri-state icon/cycle is disabled"
                );
            }
        });
    }
}
