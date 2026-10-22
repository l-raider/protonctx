//! Slint glue: connects the windows to [`State`] and runs the background
//! work (discovery, launches, the portal file chooser, shader-cache
//! deletion).
//!
//! [`App`] owns the state and weak handles to the windows; every callback
//! captures a weak reference back to it, so nothing keeps the app alive but
//! the event loop. User actions mutate the state, then [`App::sync`] pushes it
//! to the main window.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak as RcWeak};
use std::time::Duration;

use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};

use crate::background::{self, BrowseOutcome, LaunchEvent};
use crate::games::{ColumnSort, GameRow};
use crate::state::{LoadOutcome, State};
use crate::theme::AppTokens;
use crate::ui::{
    AboutDialog, DeleteCacheDialog, ErrorDialog, GameRowData, MainWindow, SettingsDialog,
};

pub type Shared = Rc<RefCell<App>>;

pub struct App {
    pub state: State,
    me: RcWeak<RefCell<App>>,
    ui: slint::Weak<MainWindow>,
    model: Rc<VecModel<GameRowData>>,
    tokens: AppTokens,
    launch_sender: async_channel::Sender<LaunchEvent>,
    /// Kept alive: on X11 the owner of a copied text serves it.
    clipboard: Option<arboard::Clipboard>,
    /// Log revision and trimmed-line count the window last showed.
    shown_log: (u64, usize),
    /// X11 windows already marked as dialogs of the main window. Hiding a
    /// dialog keeps its X11 window and the hints on it, so only its first
    /// showing needs the remap (see `parent_when_mapped`).
    parented: Rc<RefCell<HashSet<u64>>>,

    about: Option<AboutDialog>,
    settings: Option<SettingsDialog>,
    error: Option<ErrorDialog>,
    delete: Option<(DeleteCacheDialog, GameRow)>,
}

/// Build the app around `ui`, wire every callback, and start the first
/// discovery.
pub fn start(ui: &MainWindow, tokens: AppTokens) -> Shared {
    let (launch_sender, launch_receiver) =
        async_channel::bounded(background::LAUNCH_EVENT_CAPACITY);
    let model = Rc::new(VecModel::default());
    ui.set_rows(ModelRc::from(model.clone()));
    ui.set_app_title(format!("protonctx v{}", env!("CARGO_PKG_VERSION")).into());

    let shared = Rc::new_cyclic(|me| {
        RefCell::new(App {
            state: State::new(crate::config::load_config().remember_last_dir),
            me: me.clone(),
            ui: ui.as_weak(),
            model,
            tokens,
            launch_sender,
            clipboard: None,
            shown_log: (0, 0),
            parented: Rc::default(),
            about: None,
            settings: None,
            error: None,
            delete: None,
        })
    });

    wire(ui, &shared);

    // Launch output arrives in batches from the reader and watcher threads.
    let feed = Rc::downgrade(&shared);
    let _ = slint::spawn_local(async move {
        while let Some(batch) = background::next_batch(&launch_receiver).await {
            let Some(app) = feed.upgrade() else {
                break;
            };
            let mut app = app.borrow_mut();
            for event in batch {
                app.launch_event(event);
            }
            app.sync();
        }
    });

    shared.borrow_mut().refresh();
    shared
}

/// Run `f` on the app, if it still exists.
fn with_app(me: &RcWeak<RefCell<App>>, f: impl FnOnce(&mut App)) {
    if let Some(app) = me.upgrade() {
        f(&mut app.borrow_mut());
    }
}

fn wire(ui: &MainWindow, shared: &Shared) {
    let me = Rc::downgrade(shared);
    macro_rules! on {
        ($setter:ident, |$app:ident $(, $arg:ident)*| $body:expr) => {{
            let me = me.clone();
            ui.$setter(move |$($arg),*| with_app(&me, |$app| $body));
        }};
    }

    on!(on_refresh, |app| app.refresh());
    on!(on_open_settings, |app| app.open_settings());
    on!(on_clear_logs, |app| {
        app.state.log.clear();
        app.sync();
    });
    on!(on_open_about, |app| app.open_about());
    on!(on_exit_app, |app| app.exit());
    on!(on_header_clicked, |app, column| {
        app.state
            .sort_by_header(usize::try_from(column).unwrap_or(0));
        app.sync_rows();
        app.sync();
    });
    on!(on_row_pressed, |app, row| {
        app.state.select_row(usize::try_from(row).ok());
        app.sync();
    });
    on!(on_launch_tool, |app, tool| app.launch_tool(tool.as_str()));
    on!(on_browse, |app| app.browse());
    on!(on_copy_compatdata, |app| {
        if let Some(row) = app.state.selected_row() {
            let path =
                crate::steam::paths::app_compatdata_dir(Path::new(&row.library_path), row.app_id);
            app.copy(path.to_string_lossy().into_owned());
        }
    });
    on!(on_copy_tool_path, |app| {
        if let Some(row) = app.state.selected_row() {
            let path = row.proton_dir.clone();
            app.copy(path);
        }
    });
    on!(on_delete_shader_cache, |app| app
        .confirm_delete_shader_cache());

    // The selected row is filled only while the main window is focused.
    let focus = me.clone();
    ui.window().on_winit_window_event(move |_window, event| {
        if let winit::event::WindowEvent::Focused(focused) = event {
            let focused = *focused;
            with_app(&focus, |app| {
                if let Some(ui) = app.ui.upgrade() {
                    ui.set_focused(focused);
                }
            });
        }
        EventResult::Propagate
    });

    // Closing the main window exits, taking the dialogs with it.
    ui.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        CloseRequestResponse::HideWindow
    });
}

impl App {
    /// Push the state to the main window.
    fn sync(&mut self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let s = &self.state;
        ui.set_selected(
            s.selected
                .and_then(|row| i32::try_from(row).ok())
                .unwrap_or(-1),
        );
        ui.set_sort_column(i32::try_from(s.sort_column).unwrap_or(0));
        ui.set_sort_descending(s.sort == ColumnSort::Descending);
        ui.set_status_text(s.status_text.as_str().into());
        ui.set_loading(s.loading);
        ui.set_launch_running(s.launch_running);
        ui.set_cache_exists(s.selected_cache_exists);
        ui.set_dialog_open(self.dialog_open());

        let log = (s.log.revision(), s.log.trimmed_total());
        if log != self.shown_log {
            let removed = log.1.saturating_sub(self.shown_log.1);
            self.shown_log = log;
            ui.set_log_text(s.log.text().into());
            ui.set_log_lines(i32::try_from(s.log.line_count()).unwrap_or(i32::MAX));
            if removed > 0 {
                ui.invoke_log_trimmed(i32::try_from(removed).unwrap_or(i32::MAX));
            } else {
                ui.invoke_log_appended();
            }
        }
    }

    /// Push the rows to the table model.
    fn sync_rows(&mut self) {
        let rows: Vec<GameRowData> = self
            .state
            .rows
            .iter()
            .map(|row| GameRowData {
                name: row.name.as_str().into(),
                app_id: SharedString::from(row.app_id.to_string()),
                tool: row.compat_tool.as_str().into(),
            })
            .collect();
        self.model.set_vec(rows);
    }

    fn dialog_open(&self) -> bool {
        let shown = |window: &slint::Window| window.is_visible();
        self.about.as_ref().is_some_and(|d| shown(d.window()))
            || self.settings.as_ref().is_some_and(|d| shown(d.window()))
            || self.error.as_ref().is_some_and(|d| shown(d.window()))
            || self.delete.as_ref().is_some_and(|(d, _)| shown(d.window()))
    }

    fn exit(&mut self) {
        let _ = slint::quit_event_loop();
    }

    // ---- Discovery ----

    /// Re-run Steam discovery on a worker thread; ignored while one runs.
    fn refresh(&mut self) {
        if !self.state.begin_refresh() {
            return;
        }
        self.sync();
        let (sender, receiver) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("discovery".to_string())
            .spawn(move || {
                let _ =
                    sender.send_blocking(LoadOutcome::from_result(crate::steam::discover_games()));
            });
        if let Err(error) = spawned {
            self.state.games_loaded(LoadOutcome::Failed(format!(
                "Failed to discover games: could not start a thread ({error})"
            )));
            self.sync();
            return;
        }
        let me = self.me.clone();
        let _ = slint::spawn_local(async move {
            if let Ok(outcome) = receiver.recv().await {
                with_app(&me, |app| {
                    app.state.games_loaded(outcome);
                    app.sync_rows();
                    app.sync();
                });
            }
        });
    }

    // ---- Launching ----

    fn require_game(&mut self) -> Option<crate::models::Game> {
        match self.state.selected_row() {
            Some(row) => Some(row.to_game()),
            None => {
                self.show_error(Failure::Launch, "No game selected".to_string());
                None
            }
        }
    }

    fn launch_tool(&mut self, tool: &str) {
        let Some(game) = self.require_game() else {
            return;
        };
        match crate::launcher::launch_tool(&game, tool) {
            Ok(process) => self.start_launch(process, format!("Running {tool}...")),
            Err(error) => {
                self.show_error(Failure::Launch, format!("Failed to launch {tool}: {error}"))
            }
        }
    }

    fn start_launch(&mut self, mut process: crate::launcher::LaunchedProcess, status: String) {
        let generation = self.state.launch_started(status, &process.command_line);
        let (token, readers) = background::reader_tracker();
        background::stream_pipe(
            process.child.stdout.take(),
            self.launch_sender.clone(),
            token.clone(),
        );
        background::stream_pipe(
            process.child.stderr.take(),
            self.launch_sender.clone(),
            token,
        );
        if let Err(error) = background::watch_exit(
            process.child,
            generation,
            readers,
            self.launch_sender.clone(),
        ) {
            // Without the watcher the child is never reaped and the launch
            // count never drops; clear it so the spinner cannot wedge.
            self.state.watcher_spawn_failed(&error, generation);
        }
        self.sync();
    }

    fn launch_event(&mut self, event: LaunchEvent) {
        match event {
            LaunchEvent::Line { text } => {
                self.state.log.append(&text);
            }
            LaunchEvent::Exited { generation, result } => {
                self.state
                    .finish_launch(generation, background::exit_message(&result));
            }
        }
    }

    /// Pick a Windows executable through the XDG desktop portal and run it in
    /// the selected game's prefix. The portal dialog is not modal, so only
    /// one request may be in flight.
    ///
    /// The game is captured now, not when the chooser returns: the table
    /// stays usable meanwhile, so the selection can change, or a refresh can
    /// clear it, before a file is picked.
    fn browse(&mut self) {
        if self.state.browsing {
            return;
        }
        let Some(game) = self.require_game() else {
            return;
        };
        self.state.browsing = true;
        let initial_dir = if self.state.remember_last_dir {
            crate::config::load_last_dir()
        } else {
            None
        };
        let me = self.me.clone();
        let _ = slint::spawn_local(async move {
            let outcome = background::pick_executable(initial_dir).await;
            with_app(&me, |app| app.browse_finished(outcome, game));
        });
    }

    fn browse_finished(&mut self, outcome: BrowseOutcome, game: crate::models::Game) {
        self.state.browsing = false;
        match outcome {
            BrowseOutcome::Picked(path) => {
                if self.state.remember_last_dir
                    && let Some(parent) = path.parent()
                    && let Err(error) = crate::config::save_last_dir(parent)
                {
                    eprintln!("protonctx: failed to save last directory: {error}");
                }
                self.launch_browsed(path, game);
            }
            BrowseOutcome::Nothing => {}
            BrowseOutcome::Failed(error) => {
                self.show_error(
                    Failure::FileDialog,
                    format!("Failed to open file dialog: {error}"),
                );
            }
        }
    }

    /// Run a picked executable in `game`'s prefix. Resolving the pick can ask
    /// the host's document portal and block, so it runs on a worker thread;
    /// the launch itself happens back on the UI thread.
    fn launch_browsed(&mut self, path: PathBuf, game: crate::models::Game) {
        let status = format!("Running {} in {}...", path.display(), game.name);
        let (sender, receiver) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new()
            .name("resolve-executable".to_string())
            .spawn(move || {
                let _ = sender.send_blocking(crate::launcher::resolve_executable(&path));
            });
        if let Err(error) = spawned {
            self.show_error(
                Failure::Launch,
                format!("Failed to launch: could not start a thread ({error})"),
            );
            return;
        }
        let me = self.me.clone();
        let _ = slint::spawn_local(async move {
            let Ok(exe) = receiver.recv().await else {
                return;
            };
            with_app(&me, |app| {
                match crate::launcher::run_executable(&game, &exe) {
                    Ok(process) => app.start_launch(process, status),
                    Err(error) => {
                        app.show_error(Failure::Launch, format!("Failed to launch: {error}"));
                    }
                }
            });
        });
    }

    /// Write non-empty `text` to the clipboard.
    fn copy(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.clipboard = Some(clipboard),
                Err(error) => {
                    eprintln!("protonctx: no clipboard: {error}");
                    return;
                }
            }
        }
        if let Some(clipboard) = &mut self.clipboard
            && let Err(error) = clipboard.set_text(text)
        {
            eprintln!("protonctx: could not copy: {error}");
        }
    }

    // ---- Dialogs ----

    fn window_of(&self, which: Which) -> Option<&slint::Window> {
        match which {
            Which::About => self.about.as_ref().map(|d| d.window()),
            Which::Settings => self.settings.as_ref().map(|d| d.window()),
            Which::Error => self.error.as_ref().map(|d| d.window()),
            Which::Delete => self.delete.as_ref().map(|(d, _)| d.window()),
        }
    }

    /// Hide a dialog (Esc, the WM close button, or its buttons) unless it is
    /// locked; reopening reuses it.
    fn dismiss(&mut self, which: Which) {
        if which == Which::Delete && self.state.deleting_shader_cache {
            return;
        }
        if let Some(window) = self.window_of(which) {
            let _ = window.hide();
        }
        self.sync();
        if let Some(ui) = self.ui.upgrade() {
            ui.window().with_winit_window(|w| w.focus_window());
        }
    }

    /// Focus a visible dialog, or show it as a dialog of the main window.
    fn present(&mut self, which: Which) {
        let Some(window) = self.window_of(which) else {
            return;
        };
        if window.is_visible() {
            window.with_winit_window(|w| w.focus_window());
            self.sync();
            return;
        }
        // A reused dialog opens as a new one does: the focus on its default
        // button, whatever was focused when it last closed.
        self.focus_default(which);
        if let Err(error) = window.show() {
            eprintln!("protonctx: could not open a dialog: {error}");
            return;
        }
        // Without an X display the windows are native Wayland ones, which
        // cannot be parented (see `ui::select_backend`).
        let parented = window
            .with_winit_window(|w| u64::from(w.id()))
            .is_some_and(|id| self.parented.borrow().contains(&id));
        if crate::ui::has_x11_display() && !parented {
            self.parent_to_main(which);
        }
        self.sync();
    }

    /// Give a dialog's default button the keyboard focus.
    fn focus_default(&self, which: Which) {
        match which {
            Which::About => self.about.as_ref().map(AboutDialog::invoke_focus_default),
            Which::Settings => self
                .settings
                .as_ref()
                .map(SettingsDialog::invoke_focus_default),
            Which::Error => self.error.as_ref().map(ErrorDialog::invoke_focus_default),
            Which::Delete => self.delete.as_ref().map(|(d, _)| d.invoke_focus_default()),
        };
    }

    /// Mark a shown dialog as a dialog of the main window on X11.
    fn parent_to_main(&self, which: Which) {
        let main = self.ui.clone();
        let done = self.parented.clone();
        match which {
            Which::About => self
                .about
                .as_ref()
                .map(|d| parent_when_mapped(d.as_weak(), main, done, 0)),
            Which::Settings => self
                .settings
                .as_ref()
                .map(|d| parent_when_mapped(d.as_weak(), main, done, 0)),
            Which::Error => self
                .error
                .as_ref()
                .map(|d| parent_when_mapped(d.as_weak(), main, done, 0)),
            Which::Delete => self
                .delete
                .as_ref()
                .map(|(d, _)| parent_when_mapped(d.as_weak(), main, done, 0)),
        };
    }

    fn open_about(&mut self) {
        if self.about.is_none() {
            let Ok(dialog) = crate::ui::new_dialog(AboutDialog::new) else {
                return;
            };
            crate::ui::apply_theme(&dialog, &self.tokens);
            dialog.set_version(format!("protonctx v{}", env!("CARGO_PKG_VERSION")).into());
            let me = self.me.clone();
            dialog.on_ok(move || with_app(&me, |app| app.dismiss(Which::About)));
            dismiss_on_close(dialog.window(), self.me.clone(), Which::About);
            self.about = Some(dialog);
        }
        self.present(Which::About);
    }

    fn open_settings(&mut self) {
        if self.settings.is_none() {
            let Ok(dialog) = crate::ui::new_dialog(SettingsDialog::new) else {
                return;
            };
            crate::ui::apply_theme(&dialog, &self.tokens);
            let me = self.me.clone();
            dialog.on_remember_changed(move |value| {
                with_app(&me, |app| app.apply_remember_last_dir(value));
            });
            let me = self.me.clone();
            dialog.on_ok(move || with_app(&me, |app| app.dismiss(Which::Settings)));
            dismiss_on_close(dialog.window(), self.me.clone(), Which::Settings);
            self.settings = Some(dialog);
        }
        if let Some(dialog) = &self.settings {
            dialog.set_remember_last_dir(self.state.remember_last_dir);
        }
        self.present(Which::Settings);
    }

    /// Log a failure and show it in the error dialog, titled for what
    /// failed; the dialog is transient, so the log keeps the history.
    fn show_error(&mut self, failure: Failure, message: String) {
        self.state
            .log
            .append(&format!("{}: {message}", failure.log_label()));
        if self.error.is_none() {
            let Ok(dialog) = crate::ui::new_dialog(ErrorDialog::new) else {
                self.sync();
                return;
            };
            crate::ui::apply_theme(&dialog, &self.tokens);
            let me = self.me.clone();
            dialog.on_ok(move || with_app(&me, |app| app.dismiss(Which::Error)));
            dismiss_on_close(dialog.window(), self.me.clone(), Which::Error);
            self.error = Some(dialog);
        }
        if let Some(dialog) = &self.error {
            dialog.set_heading(failure.title().into());
            dialog.set_message(message.into());
        }
        self.present(Which::Error);
    }

    /// Ask before deleting the selected game's shader cache.
    fn confirm_delete_shader_cache(&mut self) {
        let Some(row) = self.state.selected_row().cloned() else {
            return;
        };
        let path = crate::steam::shadercache::shader_cache_dir_for(
            Path::new(&row.library_path),
            row.app_id,
        );
        let message = format!(
            "This will permanently delete the shader cache directory:\n\n{}\n\n\
             Shader caches are rebuilt automatically the next time the game runs, \
             but for best results close the game before deleting.",
            path.display()
        );
        if self.delete.is_none() {
            let Ok(dialog) = crate::ui::new_dialog(DeleteCacheDialog::new) else {
                return;
            };
            crate::ui::apply_theme(&dialog, &self.tokens);
            let me = self.me.clone();
            dialog.on_yes(move || with_app(&me, |app| app.commit_delete_shader_cache()));
            let me = self.me.clone();
            dialog.on_no(move || with_app(&me, |app| app.dismiss(Which::Delete)));
            // Dismissing is vetoed while the deletion runs (`dismiss`).
            dismiss_on_close(dialog.window(), self.me.clone(), Which::Delete);
            self.delete = Some((dialog, row.clone()));
        }
        if let Some((dialog, game)) = self.delete.as_mut() {
            *game = row;
            dialog.set_message(message.into());
            dialog.set_deleting(false);
        }
        self.present(Which::Delete);
    }

    /// Delete the confirmed game's cache on a worker thread; the dialog stays
    /// open, locked, until the outcome is known, then closes on every
    /// outcome.
    fn commit_delete_shader_cache(&mut self) {
        if self.state.deleting_shader_cache {
            return;
        }
        let Some((dialog, game)) = self.delete.as_ref() else {
            return;
        };
        self.state.deleting_shader_cache = true;
        dialog.set_deleting(true);
        let game = game.clone();
        let library = PathBuf::from(&game.library_path);
        let app_id = game.app_id;
        let (sender, receiver) = async_channel::bounded(1);
        let _ = std::thread::Builder::new()
            .name("delete-shader-cache".to_string())
            .spawn(move || {
                let result = crate::steam::shadercache::delete_shader_cache(&library, app_id)
                    .map_err(|error| error.to_string());
                let _ = sender.send_blocking(result);
            });
        let me = self.me.clone();
        let _ = slint::spawn_local(async move {
            let result = receiver
                .recv()
                .await
                .unwrap_or_else(|_| Err("the deletion thread stopped".to_string()));
            with_app(&me, |app| {
                let error = app.state.shader_cache_deleted(&game, result);
                if let Some((dialog, _)) = &app.delete {
                    dialog.set_deleting(false);
                }
                app.dismiss(Which::Delete);
                match error {
                    Some(message) => app.show_error(Failure::ShaderCache, message),
                    None => app.sync(),
                }
            });
        });
        self.sync();
    }

    /// Persist the settings checkbox immediately and log the change.
    fn apply_remember_last_dir(&mut self, value: bool) {
        self.state.remember_last_dir = value;
        let config = crate::config::AppConfig {
            remember_last_dir: value,
        };
        if let Err(error) = crate::config::save_config(&config) {
            eprintln!("protonctx: failed to save config: {error}");
        }
        self.state
            .log
            .append(&format!("Settings: remember last directory = {value}"));
        self.sync();
    }
}

/// What a failure shown in the error dialog was about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// Starting a process in a prefix, or nothing selected to start it in.
    Launch,
    /// The portal file chooser.
    FileDialog,
    /// Deleting a shader cache.
    ShaderCache,
}

impl Failure {
    /// The error dialog's title.
    fn title(self) -> &'static str {
        match self {
            Failure::Launch => "Launch Error",
            Failure::FileDialog => "File Dialog Error",
            Failure::ShaderCache => "Shader Cache Error",
        }
    }

    /// The label the log line starts with.
    fn log_label(self) -> &'static str {
        match self {
            Failure::Launch => "Launch error",
            Failure::FileDialog => "File dialog error",
            Failure::ShaderCache => "Shader cache error",
        }
    }
}

/// The four dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    About,
    Settings,
    Error,
    Delete,
}

/// Route the WM close button through the same dismissal as Esc.
fn dismiss_on_close(window: &slint::Window, me: RcWeak<RefCell<App>>, which: Which) {
    window.on_close_requested(move || {
        with_app(&me, |app| app.dismiss(which));
        CloseRequestResponse::KeepWindowShown
    });
}

/// Once the dialog's X11 window exists, mark it transient for the main
/// window, then unmap and map it so the window manager reads the hints as it
/// manages it (centred on and above the parent, off the taskbar). The window
/// is recorded in `done` so later showings skip this.
fn parent_when_mapped<T: ComponentHandle + 'static>(
    dialog: slint::Weak<T>,
    main: slint::Weak<MainWindow>,
    done: Rc<RefCell<HashSet<u64>>>,
    attempt: u32,
) {
    const RETRY: Duration = Duration::from_millis(5);
    const ATTEMPTS: u32 = 100;
    slint::Timer::single_shot(RETRY, move || {
        let Some(component) = dialog.upgrade() else {
            return;
        };
        let window = component.window();
        let dialog_id = window.with_winit_window(|w| u64::from(w.id()));
        let main_id = main
            .upgrade()
            .and_then(|ui| ui.window().with_winit_window(|w| u64::from(w.id())));
        match (dialog_id, main_id) {
            (Some(dialog_id), Some(main_id)) => {
                let (Ok(dialog_id), Ok(main_id)) =
                    (u32::try_from(dialog_id), u32::try_from(main_id))
                else {
                    return;
                };
                match crate::x11_dialog::mark_dialog(dialog_id, main_id) {
                    Ok(()) => {
                        done.borrow_mut().insert(u64::from(dialog_id));
                        let _ = window.hide();
                        let _ = window.show();
                    }
                    Err(error) => eprintln!("protonctx: could not parent the dialog: {error}"),
                }
            }
            _ if attempt < ATTEMPTS => parent_when_mapped(dialog, main, done, attempt + 1),
            _ => eprintln!("protonctx: the dialog window never appeared"),
        }
    });
}
