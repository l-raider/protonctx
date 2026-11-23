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

#[cfg(test)]
mod tests {
    //! The app driven end to end on a headless Slint platform with a real
    //! event loop, against a fake Steam tree under a temporary `$HOME`.
    //!
    //! The app reads and writes the user's config and Steam directories, so
    //! every scenario runs in a child process (see
    //! [`crate::test_support::run_in_child`]) whose `HOME` and XDG
    //! directories point into a temp dir, and which has no display and no
    //! D-Bus session (the portal file chooser then fails fast).

    use std::cell::RefCell;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};

    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{EventLoopProxy, Platform, WindowAdapter, WindowEvent};
    use slint::{ComponentHandle, EventLoopError, Model};

    use super::{App, BrowseOutcome, Failure, Shared, Which, start, with_app};
    use crate::test_support::{is_child, run_in_child, test_game};
    use crate::ui::MainWindow;

    // ---- A headless platform with an event loop ----

    enum Message {
        Run(Box<dyn FnOnce() + Send>),
        Quit,
    }

    struct Proxy(mpsc::Sender<Message>);

    impl EventLoopProxy for Proxy {
        fn quit_event_loop(&self) -> Result<(), EventLoopError> {
            self.0
                .send(Message::Quit)
                .map_err(|_| EventLoopError::EventLoopTerminated)
        }

        fn invoke_from_event_loop(
            &self,
            event: Box<dyn FnOnce() + Send>,
        ) -> Result<(), EventLoopError> {
            self.0
                .send(Message::Run(event))
                .map_err(|_| EventLoopError::EventLoopTerminated)
        }
    }

    /// Software-rendered windows plus an event loop that runs queued
    /// callbacks and Slint timers until it is asked to quit.
    struct Headless {
        sender: mpsc::Sender<Message>,
        receiver: Mutex<mpsc::Receiver<Message>>,
    }

    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
        }

        fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
            let receiver = self.receiver.lock().unwrap();
            loop {
                slint::platform::update_timers_and_animations();
                let wait = slint::platform::duration_until_next_timer_update()
                    .unwrap_or(Duration::from_millis(20))
                    .min(Duration::from_millis(20));
                match receiver.recv_timeout(wait) {
                    Ok(Message::Run(event)) => event(),
                    Ok(Message::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Ok(());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }

        fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
            Some(Box::new(Proxy(self.sender.clone())))
        }
    }

    fn install_platform() {
        let (sender, receiver) = mpsc::channel();
        slint::platform::set_platform(Box::new(Headless {
            sender,
            receiver: Mutex::new(receiver),
        }))
        .expect("one platform per child process");
    }

    /// Run the event loop until `done` holds, failing after a generous
    /// deadline.
    fn run_until(what: &str, done: impl Fn() -> bool + 'static) {
        let done = Rc::new(done);
        let deadline = Instant::now() + Duration::from_secs(20);
        let timer = slint::Timer::default();
        let check = done.clone();
        // Quit once: the timer can fire again before the loop reads the
        // quit, and a second one would end the next run early.
        let quitting = std::cell::Cell::new(false);
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(2),
            move || {
                if !quitting.get() && (check() || Instant::now() > deadline) {
                    quitting.set(true);
                    let _ = slint::quit_event_loop();
                }
            },
        );
        slint::run_event_loop().unwrap();
        assert!(done(), "timed out waiting for {what}");
    }

    /// Run the event loop for `duration`, letting timers and tasks run.
    fn run_for(duration: Duration) {
        let end = Instant::now() + duration;
        run_until("time to pass", move || Instant::now() >= end);
    }

    // ---- The fake Steam tree ----

    /// App id of the game with a prefix, a Proton script, and a shader cache.
    const ALPHA: u32 = 100;
    /// App id of the game that has never run under Proton.
    const BETA: u32 = 200;

    fn steam_root(home: &Path) -> PathBuf {
        home.join(".local/share/Steam")
    }

    fn manifest(steamapps: &Path, app_id: u32, name: &str, dir: &str) {
        std::fs::write(
            steamapps.join(format!("appmanifest_{app_id}.acf")),
            format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{app_id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{dir}\"\n}}\n"
            ),
        )
        .unwrap();
        std::fs::create_dir_all(steamapps.join("common").join(dir)).unwrap();
    }

    /// Build `home` with a Steam root holding two games. Alpha's prefix
    /// records a "Proton" whose script prints its arguments and exits.
    fn fake_home(home: &Path) {
        use std::os::unix::fs::PermissionsExt as _;

        let _ = std::fs::remove_dir_all(home);
        let root = steam_root(home);
        let steamapps = root.join("steamapps");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            root.join("config/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                root.display()
            ),
        )
        .unwrap();
        manifest(&steamapps, ALPHA, "Alpha Game", "Alpha");
        manifest(&steamapps, BETA, "Beta Game", "Beta");

        let proton = steamapps.join("common/FakeProton");
        std::fs::create_dir_all(&proton).unwrap();
        let script = proton.join("proton");
        std::fs::write(&script, "#!/bin/sh\necho \"proton $*\"\necho oops >&2\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let prefix = steamapps.join("compatdata").join(ALPHA.to_string());
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::write(
            prefix.join("config_info"),
            format!("11.0-100\n{}/files/share/default_pfx/\n", proton.display()),
        )
        .unwrap();

        std::fs::create_dir_all(steamapps.join("shadercache").join(ALPHA.to_string())).unwrap();
        std::fs::create_dir_all(home.join("config")).unwrap();
        std::fs::create_dir_all(home.join("state")).unwrap();
    }

    /// Run the test `name` in a child whose home is a fresh fake Steam tree.
    /// `display` is the child's `DISPLAY`, or none.
    fn in_fake_home(name: &str, tag: &str, display: Option<&str>) {
        let home = crate::test_support::temp_dir(tag);
        fake_home(&home);
        let config = home.join("config");
        let state = home.join("state");
        let mut set = vec![
            ("HOME", home.as_os_str()),
            ("XDG_CONFIG_HOME", config.as_os_str()),
            ("XDG_STATE_HOME", state.as_os_str()),
        ];
        let mut unset = vec!["WAYLAND_DISPLAY", "FLATPAK_ID"];
        // Unset, zbus would fall back to the real session bus.
        set.push(("DBUS_SESSION_BUS_ADDRESS", OsStr::new(NO_BUS)));
        match display {
            Some(display) => set.push(("DISPLAY", OsStr::new(display))),
            None => unset.push("DISPLAY"),
        }
        run_in_child(&format!("app::tests::{name}"), &set, &unset);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A session bus address with nothing listening.
    const NO_BUS: &str = "unix:path=/nonexistent/protonctx-test-bus";

    fn home() -> PathBuf {
        crate::xdg::home_dir().unwrap()
    }

    // ---- Driving the app ----

    struct Harness {
        ui: MainWindow,
        app: Shared,
    }

    impl Harness {
        /// Start the app and wait for the first discovery.
        fn start() -> Self {
            install_platform();
            let tokens =
                crate::theme::build(&crate::system_theme::preset(&Default::default(), false));
            let ui = MainWindow::new().unwrap();
            crate::ui::apply_theme(&ui, &tokens);
            let app = start(&ui, tokens);
            // Shown as in `main`: Slint quits the loop when the last visible
            // window hides, which would otherwise be any dialog.
            ui.show().unwrap();
            let harness = Harness { ui, app };
            harness.wait_loaded();
            harness
        }

        fn wait_loaded(&self) {
            let app = self.app.clone();
            run_until("discovery", move || !app.borrow().state.loading);
        }

        fn wait_launches(&self) {
            let app = self.app.clone();
            run_until("the launch to exit", move || {
                !app.borrow().state.launch_running
            });
        }

        /// Select the row showing `app_id`.
        fn select(&self, app_id: u32) {
            let rows = self.ui.get_rows();
            let row = (0..rows.row_count())
                .find(|&i| rows.row_data(i).unwrap().app_id == app_id.to_string())
                .unwrap();
            self.ui.invoke_row_pressed(i32::try_from(row).unwrap());
        }

        fn log(&self) -> String {
            self.app.borrow().state.log.text().to_string()
        }

        fn visible(&self, which: Which) -> bool {
            self.app
                .borrow()
                .window_of(which)
                .is_some_and(slint::Window::is_visible)
        }

        fn error_heading(&self) -> String {
            let app = self.app.borrow();
            app.error.as_ref().unwrap().get_heading().to_string()
        }

        fn error_message(&self) -> String {
            let app = self.app.borrow();
            app.error.as_ref().unwrap().get_message().to_string()
        }

        /// Close the error dialog with its OK button.
        fn close_error(&self) {
            let dialog = self.app.borrow().error.as_ref().unwrap().clone_strong();
            dialog.invoke_ok();
            assert!(!self.visible(Which::Error));
        }
    }

    // ---- Scenarios ----

    #[test]
    fn discovery_fills_the_table_and_header_clicks_sort_it() {
        if !is_child() {
            return in_fake_home(
                "discovery_fills_the_table_and_header_clicks_sort_it",
                "app_discovery",
                None,
            );
        }
        let h = Harness::start();
        assert_eq!(
            h.ui.get_app_title(),
            format!("protonctx v{}", env!("CARGO_PKG_VERSION"))
        );
        let names = |h: &Harness| -> Vec<String> {
            h.ui.get_rows()
                .iter()
                .map(|row| row.name.to_string())
                .collect()
        };
        assert_eq!(names(&h), ["Alpha Game", "Beta Game"]);
        assert!(!h.ui.get_loading());
        assert_eq!(h.ui.get_selected(), -1);

        // Sorting keeps the selection on its game.
        h.select(BETA);
        assert_eq!(h.ui.get_selected(), 1);
        h.ui.invoke_header_clicked(0);
        assert!(h.ui.get_sort_descending());
        assert_eq!(names(&h), ["Beta Game", "Alpha Game"]);
        assert_eq!(h.ui.get_selected(), 0);
        // A negative column is treated as the first.
        h.ui.invoke_header_clicked(-1);
        assert!(!h.ui.get_sort_descending());
        h.ui.invoke_header_clicked(1);
        assert_eq!(h.ui.get_sort_column(), 1);

        // A press outside the rows clears the selection.
        h.ui.invoke_row_pressed(-1);
        assert_eq!(h.ui.get_selected(), -1);

        // A refresh while one runs is ignored; the rows come back.
        h.ui.invoke_refresh();
        assert!(h.ui.get_loading());
        h.ui.invoke_refresh();
        assert_eq!(h.log().matches("Refreshing games...").count(), 2);
        h.wait_loaded();
        assert_eq!(h.ui.get_rows().row_count(), 2);
        assert_eq!(h.ui.get_selected(), -1);

        h.ui.invoke_clear_logs();
        assert_eq!(h.ui.get_log_text(), "");
        assert_eq!(h.ui.get_log_lines(), 0);
    }

    #[test]
    fn a_missing_steam_install_is_reported() {
        if !is_child() {
            let home = crate::test_support::temp_dir("app_no_steam");
            std::fs::create_dir_all(&home).unwrap();
            run_in_child(
                "app::tests::a_missing_steam_install_is_reported",
                &[("HOME", home.as_os_str())],
                &[
                    "DISPLAY",
                    "WAYLAND_DISPLAY",
                    "XDG_CONFIG_HOME",
                    "XDG_STATE_HOME",
                ],
            );
            let _ = std::fs::remove_dir_all(&home);
            return;
        }
        let h = Harness::start();
        assert_eq!(h.ui.get_rows().row_count(), 0);
        assert!(
            h.ui.get_status_text().to_lowercase().contains("steam"),
            "{}",
            h.ui.get_status_text()
        );
    }

    #[test]
    fn a_launched_tool_streams_its_output_to_the_log() {
        if !is_child() {
            return in_fake_home(
                "a_launched_tool_streams_its_output_to_the_log",
                "app_launch",
                None,
            );
        }
        let h = Harness::start();
        h.select(ALPHA);
        h.ui.invoke_launch_tool("winecfg".into());
        assert!(h.ui.get_launch_running());
        assert_eq!(h.ui.get_status_text(), "Running winecfg...");
        h.wait_launches();
        let log = h.log();
        assert!(log.contains("proton runinprefix winecfg"), "{log}");
        assert!(log.contains("oops"), "{log}");
        assert!(!h.ui.get_launch_running());
        assert!(h.ui.get_log_text().contains("proton runinprefix winecfg"));
    }

    #[test]
    fn launch_failures_open_the_error_dialog() {
        if !is_child() {
            return in_fake_home(
                "launch_failures_open_the_error_dialog",
                "app_launch_errors",
                None,
            );
        }
        let h = Harness::start();

        h.ui.invoke_launch_tool("winecfg".into());
        assert!(h.visible(Which::Error));
        assert!(h.ui.get_dialog_open());
        assert_eq!(h.error_heading(), "Launch Error");
        assert_eq!(h.error_message(), "No game selected");
        assert!(h.log().contains("Launch error: No game selected"));

        // A second failure reuses the open dialog.
        h.select(BETA);
        h.ui.invoke_launch_tool("winecfg".into());
        assert!(h.visible(Which::Error));
        assert!(
            h.error_message()
                .starts_with("Failed to launch winecfg: no Proton directory"),
            "{}",
            h.error_message()
        );
        h.close_error();
        assert!(!h.ui.get_dialog_open());

        // Browsing with nothing selected fails the same way.
        h.ui.invoke_row_pressed(-1);
        h.ui.invoke_browse();
        assert_eq!(h.error_message(), "No game selected");
        assert!(!h.app.borrow().state.browsing);

        // The window manager's close button dismisses as OK does.
        dispatch_close(&h, Which::Error);
        assert!(!h.visible(Which::Error));
    }

    #[test]
    fn a_failed_file_chooser_is_reported() {
        if !is_child() {
            return in_fake_home("a_failed_file_chooser_is_reported", "app_browse", None);
        }
        let h = Harness::start();
        h.select(ALPHA);
        h.ui.invoke_browse();
        assert!(h.app.borrow().state.browsing);
        // Only one chooser at a time.
        h.ui.invoke_browse();
        let app = h.app.clone();
        run_until("the chooser to fail", move || !app.borrow().state.browsing);
        assert!(h.visible(Which::Error));
        assert_eq!(h.error_heading(), "File Dialog Error");
        assert!(
            h.log()
                .contains("File dialog error: Failed to open file dialog")
        );
    }

    #[test]
    fn a_picked_executable_runs_in_the_game_prefix_and_is_remembered() {
        if !is_child() {
            return in_fake_home(
                "a_picked_executable_runs_in_the_game_prefix_and_is_remembered",
                "app_picked",
                None,
            );
        }
        let h = Harness::start();
        assert!(h.app.borrow().state.remember_last_dir);
        h.select(ALPHA);
        let game = h.app.borrow().state.selected_row().unwrap().to_game();
        let exe = home().join("Trainers/trainer.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();

        // A cancelled chooser does nothing.
        h.app
            .borrow_mut()
            .browse_finished(BrowseOutcome::Nothing, game.clone());
        assert!(!h.visible(Which::Error));

        h.app
            .borrow_mut()
            .browse_finished(BrowseOutcome::Picked(exe.clone()), game);
        let app = h.app.clone();
        run_until("the launch to start", move || {
            app.borrow().state.launch_running
        });
        assert_eq!(
            h.ui.get_status_text(),
            format!("Running {} in Alpha Game...", exe.display())
        );
        h.wait_launches();
        let log = h.log();
        assert!(
            log.contains(&format!("proton runinprefix {}", exe.display())),
            "{log}"
        );
        assert_eq!(
            crate::config::load_last_dir(),
            Some(exe.parent().unwrap().to_path_buf())
        );

        // The next chooser starts there; it fails without a portal.
        h.ui.invoke_browse();
        let app = h.app.clone();
        run_until("the chooser to fail", move || !app.borrow().state.browsing);

        // A pick for a game without a prefix cannot launch.
        h.close_error();
        let beta = test_game("Beta Game", BETA);
        let beta = crate::models::Game {
            proton_dir: String::new(),
            ..beta
        };
        h.app
            .borrow_mut()
            .browse_finished(BrowseOutcome::Picked(exe), beta);
        let app = h.app.clone();
        run_until("the launch to fail", move || {
            app.borrow()
                .error
                .as_ref()
                .is_some_and(|d| d.window().is_visible())
        });
        assert!(
            h.error_message().starts_with("Failed to launch: no Proton"),
            "{}",
            h.error_message()
        );
    }

    #[test]
    fn unwritable_config_and_state_are_logged_not_fatal() {
        if !is_child() {
            // Regular files where the XDG directories should be.
            let home = crate::test_support::temp_dir("app_unwritable");
            fake_home(&home);
            let blocker = home.join("blocker");
            std::fs::write(&blocker, "").unwrap();
            run_in_child(
                "app::tests::unwritable_config_and_state_are_logged_not_fatal",
                &[
                    ("HOME", home.as_os_str()),
                    ("XDG_CONFIG_HOME", blocker.as_os_str()),
                    ("XDG_STATE_HOME", blocker.as_os_str()),
                    ("DBUS_SESSION_BUS_ADDRESS", OsStr::new(NO_BUS)),
                ],
                &["DISPLAY", "WAYLAND_DISPLAY", "FLATPAK_ID"],
            );
            let _ = std::fs::remove_dir_all(&home);
            return;
        }
        let h = Harness::start();
        h.select(ALPHA);
        let game = h.app.borrow().state.selected_row().unwrap().to_game();
        let exe = home().join("trainer.exe");
        h.app
            .borrow_mut()
            .browse_finished(BrowseOutcome::Picked(exe), game);
        let app = h.app.clone();
        run_until("the launch to start", move || {
            app.borrow().state.launch_running
        });
        h.wait_launches();
        assert_eq!(crate::config::load_last_dir(), None);

        h.ui.invoke_open_settings();
        let settings = h.app.borrow().settings.as_ref().unwrap().clone_strong();
        settings.invoke_remember_changed(false);
        assert!(!h.app.borrow().state.remember_last_dir);
        assert!(
            h.log()
                .contains("Settings: remember last directory = false")
        );
    }

    #[test]
    fn the_settings_dialog_persists_the_checkbox() {
        if !is_child() {
            return in_fake_home(
                "the_settings_dialog_persists_the_checkbox",
                "app_settings",
                None,
            );
        }
        let h = Harness::start();
        h.ui.invoke_open_settings();
        assert!(h.visible(Which::Settings));
        assert!(h.ui.get_dialog_open());
        let settings = h.app.borrow().settings.as_ref().unwrap().clone_strong();
        assert!(settings.get_remember_last_dir());

        settings.invoke_remember_changed(false);
        assert!(!crate::config::load_config().remember_last_dir);
        assert!(
            h.log()
                .contains("Settings: remember last directory = false")
        );

        // Opening it again while it shows only focuses it.
        h.ui.invoke_open_settings();
        assert!(h.visible(Which::Settings));
        settings.invoke_ok();
        assert!(!h.visible(Which::Settings));

        // Reopening reuses the dialog and shows the current value.
        h.ui.invoke_open_settings();
        assert!(!settings.get_remember_last_dir());
        dispatch_close(&h, Which::Settings);
        assert!(!h.visible(Which::Settings));

        // Without remembering, browsing does not read the last directory.
        h.select(ALPHA);
        h.ui.invoke_browse();
        let app = h.app.clone();
        run_until("the chooser to fail", move || !app.borrow().state.browsing);
        let game = h.app.borrow().state.selected_row().unwrap().to_game();
        h.app
            .borrow_mut()
            .browse_finished(BrowseOutcome::Picked(home().join("x/trainer.exe")), game);
        h.wait_launches();
        assert_eq!(crate::config::load_last_dir(), None);
    }

    #[test]
    fn the_about_dialog_opens_and_closes() {
        if !is_child() {
            return in_fake_home("the_about_dialog_opens_and_closes", "app_about", None);
        }
        let h = Harness::start();
        h.ui.invoke_open_about();
        assert!(h.visible(Which::About));
        let about = h.app.borrow().about.as_ref().unwrap().clone_strong();
        assert_eq!(
            about.get_version(),
            format!("protonctx v{}", env!("CARGO_PKG_VERSION"))
        );
        h.ui.invoke_open_about();
        assert!(h.visible(Which::About));
        about.invoke_ok();
        assert!(!h.visible(Which::About));
        h.ui.invoke_open_about();
        assert!(h.visible(Which::About));
        dispatch_close(&h, Which::About);
        assert!(!h.visible(Which::About));

        // Exiting and closing the main window quit the event loop.
        // Each queues a quit, so each returns from a run of the loop.
        h.ui.invoke_exit_app();
        slint::run_event_loop().unwrap();
        h.ui.window().dispatch_event(WindowEvent::CloseRequested);
        slint::run_event_loop().unwrap();
    }

    #[test]
    fn the_shader_cache_is_deleted_after_confirmation() {
        if !is_child() {
            return in_fake_home(
                "the_shader_cache_is_deleted_after_confirmation",
                "app_cache",
                None,
            );
        }
        let h = Harness::start();
        // Nothing selected: nothing to confirm, nothing to commit.
        h.ui.invoke_delete_shader_cache();
        assert!(h.app.borrow().delete.is_none());
        h.app.borrow_mut().commit_delete_shader_cache();
        assert!(!h.app.borrow().state.deleting_shader_cache);

        h.select(ALPHA);
        assert!(h.ui.get_cache_exists());
        let cache = crate::steam::shadercache::shader_cache_dir_for(&steam_root(&home()), ALPHA);
        h.ui.invoke_delete_shader_cache();
        assert!(h.visible(Which::Delete));
        let dialog = h.app.borrow().delete.as_ref().unwrap().0.clone_strong();
        assert!(dialog.get_message().contains(&cache.display().to_string()));
        dialog.invoke_no();
        assert!(!h.visible(Which::Delete));
        assert!(cache.exists());

        // Confirmed: the dialog locks until the deletion finishes.
        h.ui.invoke_delete_shader_cache();
        dialog.invoke_yes();
        assert!(dialog.get_deleting());
        assert!(h.app.borrow().state.deleting_shader_cache);
        dialog.invoke_yes();
        dialog.invoke_no();
        assert!(h.visible(Which::Delete), "locked while deleting");
        let app = h.app.clone();
        run_until("the deletion", move || {
            !app.borrow().state.deleting_shader_cache
        });
        assert!(!h.visible(Which::Delete));
        assert!(!cache.exists());
        assert_eq!(h.ui.get_status_text(), "Shader cache deleted");
        assert!(!h.ui.get_cache_exists());

        // Nothing left to delete.
        h.ui.invoke_delete_shader_cache();
        dialog.invoke_yes();
        let app = h.app.clone();
        run_until("the deletion", move || {
            !app.borrow().state.deleting_shader_cache
        });
        assert_eq!(h.ui.get_status_text(), "No shader cache to delete");

        // A failed deletion is shown in the error dialog.
        h.ui.invoke_delete_shader_cache();
        if let Some((_, game)) = h.app.borrow_mut().delete.as_mut() {
            game.library_path = "relative/library".to_string();
        }
        dialog.invoke_yes();
        let app = h.app.clone();
        run_until("the deletion", move || {
            !app.borrow().state.deleting_shader_cache
        });
        assert!(h.visible(Which::Error));
        assert_eq!(h.error_heading(), "Shader Cache Error");
        assert!(!h.visible(Which::Delete));

        dispatch_close(&h, Which::Error);
        h.ui.invoke_delete_shader_cache();
        dispatch_close(&h, Which::Delete);
        assert!(!h.visible(Which::Delete));
    }

    #[test]
    fn copying_paths_without_a_clipboard_is_harmless() {
        if !is_child() {
            return in_fake_home(
                "copying_paths_without_a_clipboard_is_harmless",
                "app_copy",
                None,
            );
        }
        let h = Harness::start();
        // Nothing selected: nothing to copy.
        h.ui.invoke_copy_compatdata();
        h.ui.invoke_copy_tool_path();
        h.select(ALPHA);
        h.ui.invoke_copy_compatdata();
        h.ui.invoke_copy_tool_path();
        h.app.borrow_mut().copy(String::new());
        assert!(h.app.borrow().clipboard.is_none());
    }

    #[test]
    fn dialogs_are_parented_only_when_their_x11_window_appears() {
        // An X display that does not exist: the dialog never gets an X11
        // window, so parenting retries, then gives up.
        if !is_child() {
            return in_fake_home(
                "dialogs_are_parented_only_when_their_x11_window_appears",
                "app_x11",
                Some(":31999"),
            );
        }
        let h = Harness::start();
        h.ui.invoke_open_about();
        h.ui.invoke_open_settings();
        h.ui.invoke_launch_tool("winecfg".into());
        h.select(ALPHA);
        h.ui.invoke_delete_shader_cache();
        // 100 attempts 5 ms apart.
        run_for(Duration::from_millis(900));
        assert!(h.app.borrow().parented.borrow().is_empty());
        // The clipboard cannot reach the display either.
        h.ui.invoke_copy_tool_path();
        assert!(h.app.borrow().clipboard.is_none());

        // A dialog dropped while waiting stops the retries.
        h.app.borrow_mut().about = None;
        h.ui.invoke_open_about();
        // A shown window keeps its component alive; hide it first.
        let about = h.app.borrow_mut().about.take().unwrap();
        about.hide().unwrap();
        drop(about);
        run_for(Duration::from_millis(20));
    }

    #[test]
    fn the_log_view_follows_appends_and_trims() {
        if !is_child() {
            return in_fake_home("the_log_view_follows_appends_and_trims", "app_log", None);
        }
        let h = Harness::start();
        let lines = h.ui.get_log_lines();
        assert!(lines > 0);
        let mut app = h.app.borrow_mut();
        for i in 0..crate::log_buffer::MAX_LOG_LINES + crate::log_buffer::TRIM_SLACK {
            app.launch_event(crate::background::LaunchEvent::Line {
                text: format!("line {i}"),
            });
        }
        app.sync();
        drop(app);
        assert_eq!(
            h.ui.get_log_lines(),
            i32::try_from(crate::log_buffer::MAX_LOG_LINES).unwrap()
        );
        // An unchanged log is not copied again.
        h.app.borrow_mut().sync();
    }

    #[test]
    fn failures_have_a_title_and_a_log_label() {
        for (failure, title, label) in [
            (Failure::Launch, "Launch Error", "Launch error"),
            (
                Failure::FileDialog,
                "File Dialog Error",
                "File dialog error",
            ),
            (
                Failure::ShaderCache,
                "Shader Cache Error",
                "Shader cache error",
            ),
        ] {
            assert_eq!(failure.title(), title);
            assert_eq!(failure.log_label(), label);
        }
    }

    #[test]
    fn a_dropped_app_ignores_callbacks() {
        let gone: std::rc::Weak<RefCell<App>> = std::rc::Weak::new();
        let mut called = false;
        with_app(&gone, |_| called = true);
        assert!(!called);
    }

    /// Send the window manager's close request to a dialog.
    fn dispatch_close(h: &Harness, which: Which) {
        let app = h.app.borrow();
        let handle: Box<dyn Fn(WindowEvent)> = match which {
            Which::About => {
                let d = app.about.as_ref().unwrap().clone_strong();
                Box::new(move |e| d.window().dispatch_event(e))
            }
            Which::Settings => {
                let d = app.settings.as_ref().unwrap().clone_strong();
                Box::new(move |e| d.window().dispatch_event(e))
            }
            Which::Error => {
                let d = app.error.as_ref().unwrap().clone_strong();
                Box::new(move |e| d.window().dispatch_event(e))
            }
            Which::Delete => {
                let d = app.delete.as_ref().unwrap().0.clone_strong();
                Box::new(move |e| d.window().dispatch_event(e))
            }
        };
        drop(app);
        handle(WindowEvent::CloseRequested);
    }
}
