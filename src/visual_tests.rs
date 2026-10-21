//! Headless renders of interactive states, for comparing the port against the
//! gpui reference by eye.
//!
//! Ignored by default; run with
//! `PROTONCTX_SNAPSHOT_DIR=/some/dir cargo test visual -- --ignored` and the
//! PNGs (rendered at 2x by `iced_test`) land in that directory.

use iced::{Event, Point, mouse};
use iced_test::Simulator;

use crate::app::{Message, State};
use crate::test_support::game;

const SIZE: iced::Size = iced::Size::new(760.0, 520.0);

fn state() -> State {
    // The live desktop theme, installed before anything reads the tokens.
    let (theme, tokens) = crate::theme::build(&crate::system_theme::load(None));
    crate::theme::install(tokens);
    let mut state = State::with_main_window(iced::window::Id::unique());
    state.theme = theme;
    state.set_rows(
        [
            ("Broforce", 274190, "GE-Proton10-34"),
            ("Castle Crashers", 204360, "proton_hotfix"),
            ("Enter the Gungeon", 311690, ""),
            ("Raptor: Call of The Shadows - 2015 Edition", 336060, "proton_experimental"),
            ("The Disney Afternoon Collection", 525040, ""),
            ("Worms Crazy Golf", 70620, ""),
        ]
        .iter()
        .map(|(name, id, tool)| crate::games::GameRow::from_game(&game(name, *id, tool, "/lib", "")))
        .collect(),
    );
    // Lay the table out at the window width the sensor would report.
    let _ = state.update(Message::TableResized(iced::Size::new(SIZE.width - 8.0, 300.0)));
    let _ = state.update(Message::RowPressed(1));
    let _ = state.append_log("Refreshing games...");
    state
}

fn save(sim: &mut Simulator<'_, Message>, theme: &iced::Theme, name: &str) {
    let Some(dir) = std::env::var_os("PROTONCTX_SNAPSHOT_DIR") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(name);
    let _ = std::fs::remove_file(path.with_file_name(format!("{name}-tiny-skia.png")));
    let snapshot = sim.snapshot(theme).expect("snapshot renders");
    assert!(snapshot.matches_image(&path).expect("snapshot saves"));
}

fn move_to(sim: &mut Simulator<'_, Message>, position: Point) {
    sim.point_at(position);
    let _ = sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
}

#[test]
#[ignore]
fn visual_states() {
    let state = state();
    let theme = state.theme.clone();
    let id = state.main_window;

    // Idle with the selection, and with a hovered row and Refresh.
    let mut sim = Simulator::with_size(Default::default(), SIZE, state.view(id));
    save(&mut sim, &theme, "main-idle");
    move_to(&mut sim, Point::new(60.0, 16.0));
    save(&mut sim, &theme, "main-hover-refresh");

    // The hamburger menu, opened and hovered on its second item.
    let mut sim = Simulator::with_size(Default::default(), SIZE, state.view(id));
    move_to(&mut sim, Point::new(16.0, 16.0));
    let _ = sim.simulate(iced_test::simulator::click());
    move_to(&mut sim, Point::new(40.0, 68.0));
    save(&mut sim, &theme, "main-menu");

    // The row context menu over the third row.
    let mut sim = Simulator::with_size(Default::default(), SIZE, state.view(id));
    let row = Point::new(200.0, 33.0 + 28.0 * 2.5);
    move_to(&mut sim, row);
    let _ = sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
    move_to(&mut sim, Point::new(row.x + 30.0, row.y + 70.0));
    save(&mut sim, &theme, "main-context-menu");
}

#[test]
#[ignore]
fn visual_dialogs() {
    use crate::views::dialog_ui::{self, DialogKind};

    let state = state();
    let theme = state.theme.clone();
    let id = iced::window::Id::unique();
    let game = state.rows[2].clone();
    let kinds = [
        ("dialog-about", DialogKind::About),
        ("dialog-settings", DialogKind::Settings),
        (
            "dialog-launch-error",
            DialogKind::LaunchError(
                "Failed to launch winecfg: no Proton directory found for this game (has it been run yet?)"
                    .to_string(),
            ),
        ),
        (
            "dialog-delete-cache",
            DialogKind::DeleteShaderCache {
                game,
                path: "/home/lraider/.local/share/Steam/steamapps/shadercache/311690".into(),
                deleting: false,
            },
        ),
    ];
    for (name, kind) in kinds {
        let mut sim = Simulator::with_size(
            Default::default(),
            kind.size(),
            dialog_ui::view(id, &kind, &state),
        );
        save(&mut sim, &theme, name);
    }
}
