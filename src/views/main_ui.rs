//! Root view: toolbar, games table, action row, log pane, and status bar.
//!
//! The view owns the UI state (selection mirror, status text, launch counters)
//! and drives the unchanged backend modules (`steam`, `launcher`, `config`,
//! `flatpak`). State/action handling follows the prototype: subscriptions are
//! stored, async completions run through `update_in`, and handlers are chosen
//! per the lessons' listener table.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
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

use crate::games::{GameRow, GamesDelegate, sort_games};
use crate::log;
use crate::menu;
use crate::models::Game;
use crate::views::{about_ui, logs_ui, settings_ui};
use crate::{LogClear, LogCopy, LogSelectAll, RefreshGames};

/// Cap for the log textarea; the oldest lines are trimmed first.
pub const MAX_LOG_LINES: usize = 500;
/// Extra lines allowed before a trim runs, so trimming is not per-line work.
pub const TRIM_SLACK: usize = 50;

/// Action-row buttons: `(button id, label, tool id passed to the launcher)`.
const TOOL_BUTTONS: [(&str, &str, &str); 4] = [
    ("explorer", "Explorer", "explorer"),
    ("registry-editor", "Registry Editor", "regedit"),
    ("task-manager", "Task Manager", "taskmgr"),
    ("wine-configuration", "Wine Configuration", "winecfg"),
];

/// Events sent from the launch reader/watcher threads to the UI task.
enum LaunchEvent {
    /// A captured stdout/stderr line, or a read error from one of the pipes.
    /// Every line is logged regardless of which launch produced it, so the
    /// generation is carried for symmetry with [`LaunchEvent::Exited`].
    Line { _generation: u64, text: String },
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
    load_generation: u64,
    viewport_width: Option<Pixels>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _activation_subscription: Subscription,
    _refresh_task: Option<Task<()>>,
    _launch_events_task: Option<Task<()>>,
    _browse_task: Option<Task<()>>,
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

        let log = cx.new(|cx| TextareaState::new(window, cx));

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

        let (launch_tx, launch_rx) = async_channel::unbounded();

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
            load_generation: 0,
            viewport_width: None,
            focus_handle,
            _subscriptions: vec![subscription],
            _activation_subscription: activation_subscription,
            _refresh_task: None,
            _launch_events_task: None,
            _browse_task: None,
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
        let line = format!("[{}] {}", log::timestamp(), message);
        let insertion = if self.log_text.is_empty() {
            line
        } else {
            format!("\n{line}")
        };

        let prior_lines = self.log_text.lines().count();
        let at_bottom = self
            .log
            .read(cx)
            .visible_row_range()
            .is_none_or(|range| range.end >= prior_lines);
        let prior_scroll = self.log.read(cx).scroll_offset();
        let end = self.log_text.len();
        self.log_text.push_str(&insertion);

        self.log.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            state.insert(insertion, window, cx);
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
            let tail = self
                .log_text
                .lines()
                .skip(keep_from)
                .collect::<Vec<_>>()
                .join("\n");
            self.log_text = tail.clone();
            self.log.update(cx, |state, cx| {
                state.set_value(tail, window, cx);
            });
        }

        cx.notify();
        // Appends can arrive from background launch tasks; schedule the frame
        // explicitly (see `refresh_games` completion).
        schedule_frame(window);
    }

    /// Re-run Steam discovery, preserving the table's current sort order.
    pub(crate) fn refresh_games(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }

        self.loading = true;
        self.status_text = "Loading games...".to_string();
        cx.notify();

        // Surface the sandbox mode: inside a Flatpak, launches are routed
        // through `flatpak-spawn --host`.
        if crate::flatpak::running_in_flatpak() {
            self.append_log("Flatpak environment detected", window, cx);
        }

        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        let (sort_column, ascending) = {
            let delegate = self.table.read(cx).delegate();
            (
                delegate.sort_column,
                matches!(delegate.sort_sort, ColumnSort::Ascending),
            )
        };

        self._refresh_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Discovery is an unbounded filesystem scan; run it on a
            // background task and await the result here (Qt ran the same scan
            // on a worker thread).
            let result = cx
                .background_executor()
                .spawn(async move { crate::steam::discover_games() })
                .await;

            let _ = this.update_in(cx, |this, window, cx| {
                // A newer load superseded this one: discard the stale result.
                // The newer load owns `loading`, so leave it set.
                if !should_apply_load(generation, this.load_generation) {
                    return;
                }
                this.loading = false;

                let (rows, status, warnings) = discovery_outcome(result, sort_column, ascending);
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
                schedule_frame(window);
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
        settings_ui::open_settings_dialog(cx.weak_entity(), window, cx);
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
                schedule_frame(window);
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
        stream_pipe(stdout, generation, self.launch_tx.clone());
        stream_pipe(stderr, generation, self.launch_tx.clone());

        let tx = self.launch_tx.clone();
        let _ = std::thread::Builder::new()
            .name("launch-watcher".to_string())
            .spawn(move || {
                let result = child.wait();
                let _ = tx.send_blocking(LaunchEvent::Exited { generation, result });
            });

        cx.notify();
    }

    fn handle_launch_event(
        &mut self,
        event: LaunchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            LaunchEvent::Line { text, .. } => {
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
        window.open_dialog(cx, move |dialog, _window, cx| {
            dialog
                .title("Launch Error")
                .w(px(360.))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(Icon::new(IconName::CircleAlert).text_color(cx.theme().danger))
                        .child(div().text_sm().child(message.clone())),
                )
                .footer(
                    h_flex().w_full().justify_end().child(
                        Button::new("launch-error-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
        });
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

        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let Some(entity) = weak.upgrade() else {
                return dialog;
            };
            let on_delete = window.listener_for(&entity, |app, _, window, cx| {
                app.delete_shader_cache(window, cx);
            });

            dialog
                .title("Delete Shader Cache")
                .w(px(420.))
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .child(Icon::new(IconName::CircleAlert).text_color(cx.theme().danger))
                        .child(div().text_sm().child(message.clone())),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("delete-cache-cancel")
                                .outline()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("delete-cache-confirm")
                                .danger()
                                .label("Delete")
                                .on_click(on_delete),
                        ),
                )
        });
    }

    /// Delete the selected game's shader cache and report the outcome exactly
    /// as the legacy model did.
    fn delete_shader_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row_data(cx) else {
            self.show_launch_error("No game selected", window, cx);
            return;
        };

        let library = std::path::Path::new(&row.library_path);
        match crate::steam::shadercache::delete_shader_cache(library, row.app_id) {
            Ok(true) => {
                self.append_log(
                    format!(
                        "Deleted shader cache for {} (app id {})",
                        row.name, row.app_id
                    ),
                    window,
                    cx,
                );
                self.status_text = "Shader cache deleted".to_string();
                cx.notify();
            }
            Ok(false) => {
                self.append_log(
                    format!(
                        "No shader cache found for {} (app id {})",
                        row.name, row.app_id
                    ),
                    window,
                    cx,
                );
                self.status_text = "No shader cache to delete".to_string();
                cx.notify();
            }
            Err(e) => {
                self.show_launch_error(format!("Failed to delete shader cache: {e}"), window, cx);
            }
        }
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
        let hover_bg = input.mix_oklab(cx.theme().transparent, 0.5);
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
                    .hover(move |style| style.border_color(primary).bg(hover_bg))
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
fn schedule_frame(window: &mut Window) {
    window.on_next_frame(|window, _cx| window.refresh());
}

/// Stream one captured pipe (stdout or stderr) of a launched child to the log.
///
/// Runs on its own thread: reads `reader` line-by-line (blocking until the
/// child closes the pipe) and forwards each line through `tx`.
fn stream_pipe<R: std::io::Read + Send + 'static>(
    reader: Option<R>,
    generation: u64,
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
                            _generation: generation,
                            text: format!("read error: {e}"),
                        });
                        break;
                    }
                };
                let _ = tx.send_blocking(LaunchEvent::Line {
                    _generation: generation,
                    text,
                });
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

/// Whether a completion for `generation` still belongs to the newest load.
fn should_apply_load(generation: u64, current: u64) -> bool {
    generation == current
}

/// Map a discovery result to rows, status text, and log warnings, sorting the
/// rows with the caller's captured sort state (Qt's `load_games` contract).
fn discovery_outcome(
    result: Result<crate::steam::Discovery, crate::steam::SteamError>,
    sort_column: usize,
    ascending: bool,
) -> (Vec<GameRow>, String, Vec<String>) {
    match result {
        Ok(discovery) => {
            let mut rows: Vec<GameRow> = discovery.games.iter().map(GameRow::from_game).collect();
            sort_games(&mut rows, sort_column, ascending);

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
    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, px, size};

    use super::{ProtonctxApp, TRIM_SLACK, discovery_outcome, should_apply_load};
    use crate::models::Game;

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
    fn stale_load_generations_are_discarded() {
        assert!(should_apply_load(2, 2));
        assert!(!should_apply_load(1, 2));
        assert!(!should_apply_load(3, 2));
    }

    #[test]
    fn discovery_outcome_sorts_with_the_captured_state_and_maps_errors() {
        let discovery = crate::steam::Discovery {
            games: vec![
                test_game("Castle Crashers", 204360),
                test_game("Broforce", 274190),
            ],
            warnings: vec!["a warning".to_string()],
        };
        let (rows, status, warnings) = discovery_outcome(Ok(discovery), 1, false);
        assert_eq!(
            rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
            vec![274190, 204360]
        );
        assert_eq!(status, "2 games loaded");
        assert_eq!(warnings, vec!["a warning".to_string()]);

        let (rows, status, warnings) =
            discovery_outcome(Err(crate::steam::SteamError::SteamNotFound), 0, true);
        assert!(rows.is_empty());
        assert_eq!(status, "Steam not found");
        assert!(warnings.is_empty());

        let error = crate::steam::SteamError::Parse("bad vdf".to_string());
        let (rows, status, warnings) = discovery_outcome(Err(error), 0, true);
        assert!(rows.is_empty());
        assert_eq!(status, "Failed to discover games: parse error: bad vdf");
        assert_eq!(warnings.len(), 1);
    }

    #[gpui_kit::test]
    fn settings_menu_item_opens_dialog_and_ok_closes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let mut view = None;
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let _view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("enter", cx);
            window.render_frame(cx);
            assert!(window.has_active_dialog(cx));

            window.click("settings-ok", cx);
            assert!(!window.has_active_dialog(cx));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn about_menu_item_opens_dialog(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            Root::new(app, window, cx)
        });

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("enter", cx);
            assert!(window.has_active_dialog(cx));
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
        let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
            let app = cx.new(|cx| ProtonctxApp::new(window, cx));
            view = Some(app.clone());
            Root::new(app, window, cx)
        });
        let view = view.unwrap();

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("main-menu", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("enter", cx);
            window.render_frame(cx);
            assert!(window.has_active_dialog(cx));

            // The temp XDG root has no config, so the default (true) is shown;
            // toggling writes `false`.
            window.press("tab", cx);
            window.press("space", cx);
            assert!(!view.read(cx).remember_last_directory);

            window.click("settings-ok", cx);
            assert!(!window.has_active_dialog(cx));
        })
        .unwrap();

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
                    "] line {}",
                    super::MAX_LOG_LINES + TRIM_SLACK + 19
                )),
                "newest line was trimmed"
            );
        })
        .unwrap();
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
}
