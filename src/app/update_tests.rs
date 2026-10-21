//! `State::update` tests: the behaviour the gpui UI tests covered, driven
//! through messages instead of a test window.

use iced::{keyboard, window};

use super::{LoadOutcome, Message, WindowInput, discovery_outcome};
use crate::games::{ColumnSort, GameRow};
use crate::test_support::{state, state_with, test_game};
use crate::views::dialog_ui::{DialogButton, DialogKind};
use crate::views::logs_ui::{MAX_LOG_LINES, TRIM_SLACK};

fn key(state: &mut super::State, window: window::Id, key: keyboard::Key, command: bool) -> usize {
    let modifiers = if command {
        keyboard::Modifiers::CTRL
    } else {
        keyboard::Modifiers::empty()
    };
    state
        .update(Message::Window(window, WindowInput::Key { key, modifiers }))
        .units()
}

fn character(c: &str) -> keyboard::Key {
    keyboard::Key::Character(c.into())
}

/// The dialog window currently showing `kind`'s kind.
fn dialog(state: &super::State, kind: &DialogKind) -> window::Id {
    state
        .dialog_window(kind)
        .unwrap_or_else(|| panic!("no open {} dialog", kind.title()))
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

    let (rows, status, warnings) = discovery_outcome(
        LoadOutcome::from_result(Err(crate::steam::SteamError::SteamNotFound)),
        0,
        ColumnSort::Ascending,
    );
    assert!(rows.is_empty());
    assert_eq!(status, "Steam not found");
    assert!(warnings.is_empty());

    let error = crate::steam::SteamError::Parse("bad vdf".to_string());
    let (rows, status, warnings) = discovery_outcome(
        LoadOutcome::from_result(Err(error)),
        0,
        ColumnSort::Ascending,
    );
    assert!(rows.is_empty());
    assert_eq!(status, "Failed to discover games: parse error: bad vdf");
    assert_eq!(warnings.len(), 1);
}

#[test]
fn discovery_outcome_counts_and_keeps_order_for_default_sort() {
    let one = LoadOutcome::Found {
        rows: vec![GameRow::from_game(&test_game("Solo", 1))],
        warnings: Vec::new(),
    };
    assert_eq!(
        discovery_outcome(one, 0, ColumnSort::Ascending).1,
        "1 game loaded"
    );
    let none = LoadOutcome::Found {
        rows: Vec::new(),
        warnings: Vec::new(),
    };
    assert_eq!(
        discovery_outcome(none, 0, ColumnSort::Ascending).1,
        "No games found"
    );

    // `Default` is the neutral header state: discovery order is preserved.
    let discovery = LoadOutcome::Found {
        rows: vec![
            GameRow::from_game(&test_game("Castle Crashers", 204360)),
            GameRow::from_game(&test_game("Broforce", 274190)),
        ],
        warnings: Vec::new(),
    };
    let (rows, _, _) = discovery_outcome(discovery, 0, ColumnSort::Default);
    assert_eq!(
        rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
        vec![204360, 274190]
    );
}

#[test]
fn refresh_is_single_flight_logs_and_applies_the_current_sort() {
    let mut state = state();
    let _ = state.update(Message::Refresh);
    assert!(state.loading);
    assert_eq!(state.status_text, "Loading games...");
    assert!(state.log.text().contains("] Refreshing games...\n"));

    // A second refresh while loading is ignored.
    let _ = state.update(Message::Refresh);
    assert_eq!(state.log.text().matches("Refreshing games...").count(), 1);

    // A header click during the scan wins over the order the scan returns.
    let _ = state.update(Message::HeaderPressed(1));
    let _ = state.update(Message::GamesLoaded(LoadOutcome::Found {
        rows: vec![
            GameRow::from_game(&test_game("B", 2)),
            GameRow::from_game(&test_game("A", 1)),
        ],
        warnings: vec!["prefix unreadable".to_string()],
    }));
    assert!(!state.loading);
    assert_eq!(state.status_text, "2 games loaded");
    assert_eq!(
        state.rows.iter().map(|row| row.app_id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(state.log.text().contains("] prefix unreadable\n"));
    assert_eq!(state.selected, None);
}

#[test]
fn header_click_toggles_two_state_sort() {
    // Name-ascending discovery order matches the initial sort state
    // (column 0, ascending).
    let mut state = state_with(&[
        test_game("Broforce", 274190),
        test_game("Castle Crashers", 204360),
    ]);
    let click = |state: &mut super::State, column, sort, ids: &[u32]| {
        let _ = state.update(Message::HeaderPressed(column));
        assert_eq!(state.sort_column, column);
        assert_eq!(state.sort, sort);
        let actual: Vec<u32> = state.rows.iter().map(|row| row.app_id).collect();
        assert_eq!(actual, ids);
    };

    // Click 1: the active column toggles to descending.
    click(&mut state, 0, ColumnSort::Descending, &[204360, 274190]);
    // Click 2: and back.
    click(&mut state, 0, ColumnSort::Ascending, &[274190, 204360]);
    // Click 3: a newly clicked column starts ascending (Qt default).
    click(&mut state, 1, ColumnSort::Ascending, &[204360, 274190]);
    // Click 4: further clicks on the active column toggle direction.
    click(&mut state, 1, ColumnSort::Descending, &[274190, 204360]);
    // Click 5: returning to a column restarts ascending.
    click(&mut state, 0, ColumnSort::Ascending, &[274190, 204360]);
}

#[test]
fn sort_restores_the_selection_by_app_id() {
    let mut state = state_with(&[test_game("Alpha", 30), test_game("Beta", 10)]);
    let _ = state.update(Message::RowPressed(0));
    assert_eq!(state.selected_row().map(|row| row.app_id), Some(30));
    let _ = state.update(Message::HeaderPressed(1));
    assert_eq!(state.selected, Some(1));
    assert_eq!(state.selected_row().map(|row| row.app_id), Some(30));
}

#[test]
fn right_click_selects_the_hovered_row_and_hover_tracks_cells() {
    let mut state = state_with(&[test_game("A", 1), test_game("B", 2)]);
    let main = state.main_window;
    let _ = state.update(Message::CellEntered(1, 0));
    // Leaving one cell after entering the next keeps the newer hover.
    let _ = state.update(Message::CellEntered(1, 1));
    let _ = state.update(Message::CellExited(1, 0));
    assert_eq!(state.hovered, Some((1, 1)));
    let _ = state.update(Message::Window(main, WindowInput::RightPressed));
    assert_eq!(state.selected, Some(1));
    assert_eq!(state.columns[0].view.hovered, Some(1));
    assert_eq!(state.columns[2].view.selected, Some(1));
}

#[test]
fn the_selected_row_fill_follows_main_window_focus() {
    let mut state = state_with(&[test_game("A", 1)]);
    let main = state.main_window;
    let _ = state.update(Message::RowPressed(0));
    let _ = state.update(Message::Window(main, WindowInput::Unfocused));
    assert!(!state.columns[0].view.focused);
    let _ = state.update(Message::Window(main, WindowInput::Focused));
    assert!(state.columns[0].view.focused);
}

#[test]
fn only_the_name_and_app_id_columns_resize() {
    let mut state = state_with(&[test_game("A", 1)]);
    let _ = state.update(Message::TableResized(iced::Size::new(752.0, 300.0)));
    assert_eq!(state.columns[2].width, 752.0 - 260.0 - 110.0);

    let _ = state.update(Message::ColumnDragged(0, 40.0));
    let _ = state.update(Message::ColumnDragged(2, 40.0));
    let _ = state.update(Message::ColumnReleased);
    assert_eq!(state.columns[0].width, 300.0);
    // The compat column takes what is left, not the drag.
    assert_eq!(state.columns[2].width, 752.0 - 300.0 - 110.0);

    // Columns never shrink below the minimum.
    let _ = state.update(Message::ColumnDragged(1, -500.0));
    let _ = state.update(Message::ColumnReleased);
    assert_eq!(
        state.columns[1].width,
        crate::views::table::MIN_COLUMN_WIDTH
    );
}

#[test]
fn split_ratio_keeps_both_pane_minimums() {
    let mut state = state();
    let _ = state.update(Message::SplitResized(iced::Size::new(754.0, 455.0)));
    assert_eq!(state.split_ratio, 0.75);
    let Some(split) = state.split else {
        panic!("the main split exists");
    };
    let _ = state.update(Message::PaneResized(iced::widget::pane_grid::ResizeEvent {
        split,
        ratio: 0.99,
    }));
    assert_eq!(state.split_ratio, 1.0 - super::LOG_PANE_MIN / 455.0);
    let _ = state.update(Message::PaneResized(iced::widget::pane_grid::ResizeEvent {
        split,
        ratio: 0.01,
    }));
    assert_eq!(state.split_ratio, super::TABLE_PANE_MIN / 455.0);
}

#[test]
fn action_buttons_need_a_selection() {
    let mut state = state();
    let _ = state.update(Message::LaunchTool("winecfg"));
    assert!(
        state
            .log
            .text()
            .contains("] Launch error: No game selected\n")
    );
    assert_eq!(
        state.dialogs.values().next(),
        Some(&DialogKind::LaunchError("No game selected".to_string()))
    );
}

#[test]
fn launching_without_a_proton_dir_reports_the_error() {
    let mut game = test_game("No Prefix", 7);
    game.proton_dir = String::new();
    let mut state = state_with(&[game]);
    let _ = state.update(Message::RowPressed(0));
    let _ = state.update(Message::LaunchTool("winecfg"));
    assert!(!state.launch_running);
    let expected = "Failed to launch winecfg: no Proton directory found for this game \
                    (has it been run yet?)";
    assert!(
        state
            .log
            .text()
            .contains(&format!("] Launch error: {expected}\n"))
    );
    assert_eq!(
        state.dialogs.values().next(),
        Some(&DialogKind::LaunchError(expected.to_string()))
    );
}

#[test]
fn watcher_spawn_failure_clears_launch_state() {
    let mut state = state();
    state.active_launches = 1;
    state.launch_running = true;
    state.launch_generation = 7;

    let _ = state.handle_watcher_spawn_failure(
        &std::io::Error::other("resource temporarily unavailable"),
        7,
    );

    assert_eq!(state.active_launches, 0);
    assert!(!state.launch_running);
    assert!(state.status_text.contains("failed to start watcher"));
    assert!(state.log.text().contains("failed to start watcher"));
}

#[test]
fn append_log_caps_line_count() {
    let mut state = state();
    let total = MAX_LOG_LINES + TRIM_SLACK + 20;
    for line in 0..total {
        let _ = state.append_log(format!("line {line}"));
    }
    let lines = state.log.line_count();
    assert!(
        (MAX_LOG_LINES..=MAX_LOG_LINES + TRIM_SLACK).contains(&lines),
        "expected {MAX_LOG_LINES}..={} lines after trim, got {lines}",
        MAX_LOG_LINES + TRIM_SLACK
    );
    let text = state.log.text();
    assert_eq!(text.lines().count(), lines);
    assert_eq!(
        state.log.content.line_count(),
        lines + 1,
        "editor mirrors the shadow"
    );
    assert!(!text.contains("] line 0\n"), "oldest line was not trimmed");
    assert!(
        text.ends_with(&format!("] line {}\n", total - 1)),
        "newest line was trimmed"
    );
}

#[test]
fn log_follows_the_bottom_until_scrolled_up() {
    use iced::widget::scrollable::Viewport;

    let mut state = state();
    assert!(state.log.is_following());
    // A follow task snaps to the bottom after each append.
    assert_eq!(state.append_log("first").units(), 1);

    // iced builds the viewport; take real ones from a scrollable scrolled a
    // little (not at the bottom) and all the way down.
    state.log.scrolled(scrolled_viewport(-10.0));
    assert!(!state.log.is_following());
    assert_eq!(
        state.append_log("second").units(),
        0,
        "no snap when scrolled up"
    );

    state.log.scrolled(scrolled_viewport(-100_000.0));
    assert!(state.log.is_following());
    assert_eq!(state.append_log("third").units(), 1);

    /// The viewport a 100 px scrollable over taller content reports after a
    /// wheel scroll of `delta` pixels (negative scrolls down).
    fn scrolled_viewport(delta: f32) -> Viewport {
        use iced::widget::{column, scrollable, text};
        let lines = column((0..100).map(|line| text(line.to_string()).into()));
        let view: iced::Element<'_, Viewport> = scrollable(lines)
            .width(200)
            .height(100)
            .on_scroll(|viewport| viewport)
            .into();
        let mut sim = iced_test::Simulator::with_size(
            Default::default(),
            iced::Size::new(200.0, 100.0),
            view,
        );
        sim.point_at(iced::Point::new(50.0, 50.0));
        let redraw = || {
            iced::Event::Window(iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            ))
        };
        let _ = sim.simulate([
            redraw(),
            iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: iced::Point::new(50.0, 50.0),
            }),
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: delta },
            }),
            redraw(),
        ]);
        sim.into_messages()
            .last()
            .expect("the scrollable reported its viewport")
    }
}

#[test]
fn log_edits_are_dropped_but_selection_works() {
    use iced::widget::text_editor::{Action, Edit};

    let mut state = state();
    let _ = state.append_log("seed line");
    let before = state.log.text().to_string();
    let _ = state.update(Message::LogAction(Action::Edit(Edit::Insert('x'))));
    let _ = state.update(Message::LogAction(Action::Edit(Edit::Backspace)));
    assert_eq!(state.log.content.text(), before);
    let _ = state.update(Message::LogAction(Action::SelectAll));
    assert!(
        state
            .log
            .selection()
            .is_some_and(|text| text.contains("seed line"))
    );
}

#[test]
fn shortcuts_refresh_select_copy_and_clear() {
    let mut state = state();
    let main = state.main_window;

    // F5 refreshes.
    assert!(
        key(
            &mut state,
            main,
            keyboard::Key::Named(keyboard::key::Named::F5),
            false
        ) > 0
    );
    assert!(state.loading);

    // Ctrl+A selects the whole log and focuses it.
    assert_eq!(key(&mut state, main, character("a"), true), 1, "focus task");
    let selected = state.log.selection().unwrap_or_default();
    assert!(selected.contains("Refreshing games..."), "{selected:?}");

    // Ctrl+C copies the selection (a clipboard write task).
    assert_eq!(key(&mut state, main, character("c"), true), 1);

    // Ctrl+L clears the log.
    let _ = key(&mut state, main, character("l"), true);
    assert!(state.log.text().is_empty());
    assert!(state.log.content.text().trim().is_empty());
    assert_eq!(state.log.selection(), None);
    // Nothing selected: Ctrl+C has nothing to copy.
    assert_eq!(key(&mut state, main, character("c"), true), 0);
}

#[test]
fn shortcuts_are_ignored_while_a_dialog_is_open() {
    let mut state = state();
    let main = state.main_window;
    let _ = state.update(Message::OpenAbout);
    let _ = key(
        &mut state,
        main,
        keyboard::Key::Named(keyboard::key::Named::F5),
        false,
    );
    assert!(!state.loading);
}

#[test]
fn dialogs_open_dedupe_replace_and_close() {
    let mut state = state();
    let _ = state.update(Message::OpenAbout);
    let about = dialog(&state, &DialogKind::About);
    assert_eq!(state.title(about), "About protonctx");

    // Reopening focuses the same window instead of stacking another.
    assert_eq!(state.update(Message::OpenAbout).units(), 1);
    assert_eq!(state.dialogs.len(), 1);
    assert_eq!(dialog(&state, &DialogKind::About), about);

    // A second error replaces the first one's message in the same window.
    let _ = state.show_error("first failure");
    let error = dialog(&state, &DialogKind::LaunchError(String::new()));
    let _ = state.show_error("short failure");
    assert_eq!(
        dialog(&state, &DialogKind::LaunchError(String::new())),
        error
    );
    assert_eq!(
        state.dialogs.get(&error),
        Some(&DialogKind::LaunchError("short failure".to_string()))
    );
    assert!(state.log.text().contains("] Launch error: short failure\n"));
    assert_eq!(state.dialogs.len(), 2);

    // OK closes; Esc and the WM close button dismiss.
    assert_eq!(
        state
            .update(Message::DialogButton(about, DialogButton::Ok))
            .units(),
        1
    );
    assert!(state.dialog_window(&DialogKind::About).is_none());
    let _ = key(
        &mut state,
        error,
        keyboard::Key::Named(keyboard::key::Named::Escape),
        false,
    );
    assert!(state.dialogs.is_empty());

    let _ = state.update(Message::OpenSettings);
    let settings = dialog(&state, &DialogKind::Settings);
    let _ = state.update(Message::Window(settings, WindowInput::CloseRequested));
    assert!(state.dialogs.is_empty());

    // A window the compositor closed is dropped from the registry.
    let _ = state.update(Message::OpenAbout);
    let about = dialog(&state, &DialogKind::About);
    let _ = state.update(Message::Window(about, WindowInput::Closed));
    assert!(state.dialogs.is_empty());
}

#[test]
fn enter_and_space_activate_the_default_button() {
    let mut state = state();
    let _ = state.update(Message::OpenAbout);
    let about = dialog(&state, &DialogKind::About);
    let _ = key(
        &mut state,
        about,
        keyboard::Key::Named(keyboard::key::Named::Enter),
        false,
    );
    assert!(state.dialogs.is_empty(), "Enter activated the default OK");

    let _ = state.show_error("Failed to launch winecfg");
    let error = dialog(&state, &DialogKind::LaunchError(String::new()));
    let _ = key(
        &mut state,
        error,
        keyboard::Key::Named(keyboard::key::Named::Space),
        false,
    );
    assert!(state.dialogs.is_empty(), "Space activated the default OK");
}

#[test]
fn closing_the_main_window_exits_with_its_dialogs() {
    let mut state = state();
    let main = state.main_window;
    let _ = state.update(Message::OpenAbout);
    // `iced::exit` ends the daemon, which closes every dialog window.
    assert_eq!(
        state
            .update(Message::Window(main, WindowInput::Closed))
            .units(),
        1
    );
    // Exit closes the main window, which then exits as above.
    assert_eq!(state.update(Message::Exit).units(), 1);
}

#[test]
fn delete_shader_cache_deletes_closes_and_reports() {
    let root = crate::test_support::temp_dir("delete");
    let app_id = 274190;
    let cache = crate::steam::shadercache::shader_cache_dir_for(&root, app_id);
    std::fs::create_dir_all(cache.join("fozpipelinesv6")).unwrap();

    let mut game = test_game("Cache Test", app_id);
    game.library_path = root.to_string_lossy().into_owned();
    let mut state = state_with(&[game]);
    let _ = state.update(Message::RowPressed(0));
    assert!(
        state.selected_cache_exists,
        "the context-menu item is enabled"
    );

    let _ = state.update(Message::ConfirmDeleteShaderCache);
    let confirm = state
        .dialogs
        .iter()
        .find(|(_, kind)| matches!(kind, DialogKind::DeleteShaderCache { .. }))
        .map(|(id, _)| *id)
        .expect("the confirmation opens");
    assert_eq!(state.title(confirm), "Delete Shader Cache");

    // Yes starts the deletion: the dialog locks and repeat clicks, Esc, and
    // the WM close button are inert.
    assert_eq!(
        state
            .update(Message::DialogButton(confirm, DialogButton::Yes))
            .units(),
        1
    );
    assert!(state.deleting_shader_cache);
    assert!(matches!(
        state.dialogs.get(&confirm),
        Some(DialogKind::DeleteShaderCache { deleting: true, .. })
    ));
    assert_eq!(
        state
            .update(Message::DialogButton(confirm, DialogButton::Yes))
            .units(),
        0
    );
    let _ = key(
        &mut state,
        confirm,
        keyboard::Key::Named(keyboard::key::Named::Escape),
        false,
    );
    let _ = state.update(Message::Window(confirm, WindowInput::CloseRequested));
    assert!(
        state.dialogs.contains_key(&confirm),
        "the locked confirm stays open"
    );

    // The background deletion, run here, then its completion.
    let result = crate::steam::shadercache::delete_shader_cache(&root, app_id)
        .map_err(|error| error.to_string());
    let _ = state.update(Message::ShaderCacheDeleted(result));
    assert!(!state.deleting_shader_cache);
    assert!(
        state.dialogs.is_empty(),
        "the confirm closes after deletion"
    );
    assert_eq!(state.status_text, "Shader cache deleted");
    assert!(
        state
            .log
            .text()
            .contains("Deleted shader cache for Cache Test (app id 274190)")
    );
    assert!(!cache.exists(), "cache directory survived the delete");
    assert!(
        !state.selected_cache_exists,
        "the context-menu item is disabled again"
    );

    // Nothing left to delete: the other success outcome.
    let _ = state.update(Message::ConfirmDeleteShaderCache);
    let confirm = state.dialogs.keys().next().copied().unwrap();
    let _ = state.update(Message::DialogButton(confirm, DialogButton::Yes));
    let result = crate::steam::shadercache::delete_shader_cache(&root, app_id)
        .map_err(|error| error.to_string());
    assert_eq!(result, Ok(false));
    let _ = state.update(Message::ShaderCacheDeleted(result));
    assert!(state.dialogs.is_empty());
    assert_eq!(state.status_text, "No shader cache to delete");
    assert!(
        state
            .log
            .text()
            .contains("No shader cache found for Cache Test (app id 274190)")
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn delete_shader_cache_failure_closes_confirm_and_opens_error() {
    // A relative library path is rejected by the shadercache guard, so the
    // deletion fails without touching the filesystem.
    let mut game = test_game("Relative", 274190);
    game.library_path = "relative-lib".to_string();
    let mut state = state_with(&[game]);
    let _ = state.update(Message::RowPressed(0));
    let _ = state.update(Message::ConfirmDeleteShaderCache);
    let confirm = state.dialogs.keys().next().copied().unwrap();
    let _ = state.update(Message::DialogButton(confirm, DialogButton::Yes));

    let result = crate::steam::shadercache::delete_shader_cache(
        std::path::Path::new("relative-lib"),
        274190,
    )
    .map_err(|error| error.to_string());
    assert!(result.is_err());
    let _ = state.update(Message::ShaderCacheDeleted(result));

    assert!(!state.deleting_shader_cache);
    assert!(!state.dialogs.contains_key(&confirm), "the confirm closed");
    let error = state.dialogs.values().next();
    assert!(
        matches!(error, Some(DialogKind::LaunchError(message))
            if message.starts_with("Failed to delete shader cache: ")),
        "{error:?}"
    );
}

#[test]
fn delete_shader_cache_no_defaults_and_needs_a_selection() {
    // Without a selection there is nothing to confirm.
    let mut state = state();
    let _ = state.update(Message::ConfirmDeleteShaderCache);
    assert!(state.dialogs.is_empty());

    // No (the default, also Enter) closes without deleting.
    let mut state = state_with(&[test_game("A", 1)]);
    let _ = state.update(Message::RowPressed(0));
    let _ = state.update(Message::ConfirmDeleteShaderCache);
    let confirm = state.dialogs.keys().next().copied().unwrap();
    let _ = key(
        &mut state,
        confirm,
        keyboard::Key::Named(keyboard::key::Named::Enter),
        false,
    );
    assert!(state.dialogs.is_empty());
    assert!(!state.deleting_shader_cache);
}

#[test]
fn app_uses_the_statically_installed_theme() {
    let t = crate::theme::tokens();
    // Before any install the tokens are the deterministic Breeze Dark
    // preset; the platform appearance is never consulted at runtime.
    assert_eq!(
        t.background,
        iced::Color::from_rgb8(0x20, 0x23, 0x26),
        "window background"
    );
    // Breeze keeps scrollbars visible: iced never hides an overflowing
    // scrollable's rail, and the rail is the Breeze 16 px groove.
    let style = crate::views::widgets::scrollable_style(
        &iced::Theme::Dark,
        iced::widget::scrollable::Status::Active {
            is_horizontal_scrollbar_disabled: false,
            is_vertical_scrollbar_disabled: false,
        },
    );
    assert_eq!(
        style.vertical_rail.background,
        Some(iced::Background::Color(t.scrollbar_track))
    );
    assert_eq!(t.scrollbar_width, 16.0);
    let state = state();
    assert!(state.theme(state.main_window).is_some());
}

/// The settings checkbox persists immediately when toggled. Driven by
/// [`settings_persist_config_subprocess`] through a child process with
/// `XDG_CONFIG_HOME` pointed at a temporary directory; running it in the
/// parent would write the developer's real config, so it skips unless the
/// child's marker variable is present.
#[test]
fn settings_checkbox_persists_config_to_xdg_config_home() {
    let Some(config_home) = std::env::var_os("PROTONCTX_TEST_XDG_CONFIG_HOME") else {
        return;
    };
    let mut state = state();
    // The temp XDG root has no config, so the default (true) is shown.
    assert!(state.remember_last_dir);
    let _ = state.update(Message::OpenSettings);
    let _ = state.update(Message::RememberLastDir(false));
    assert!(!state.remember_last_dir);
    assert!(
        state
            .log
            .text()
            .contains("] Settings: remember last directory = false\n")
    );
    let settings = state.dialogs.keys().next().copied().unwrap();
    let _ = state.update(Message::DialogButton(settings, DialogButton::Ok));
    assert!(state.dialogs.is_empty());

    let config_path = std::path::Path::new(&config_home)
        .join("protonctx")
        .join("config.json");
    let text = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|e| panic!("{}: {e}", config_path.display()));
    assert!(text.contains("\"remember_last_dir\": false"), "{text}");
}

#[test]
fn settings_persist_config_subprocess() {
    let config_home = crate::test_support::temp_dir("xdg");
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
