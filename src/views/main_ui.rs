//! The main window: toolbar, games table, action row, log pane, and status
//! bar.

use iced::widget::{Space, column, container, pane_grid, row, sensor};
use iced::{Alignment, Background, Element, Length, Theme};

use crate::app::{Message, PaneKind, State};
use crate::assets::Icon;
use crate::games::TOOL_BUTTONS;
use crate::theme::tokens;
use crate::views::widgets::{self, icon, label};
use crate::views::{logs_ui, table};

/// Gap between the table pane and the log pane; the split handle inside it
/// is invisible.
pub const SPLIT_GAP: f32 = 0.0;
/// Status bar height below its 1 px top rule.
const STATUS_BAR_HEIGHT: f32 = 28.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let t = tokens();
    let edge = t.rem(1.0);
    let content = container(split(state))
        .padding(iced::Padding {
            top: 0.0,
            right: edge,
            bottom: edge,
            left: edge,
        })
        .width(Length::Fill)
        .height(Length::Fill);

    container(column![toolbar(state), content, status_bar(state)])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.background)),
            text_color: Some(t.foreground),
            ..container::Style::default()
        })
        .into()
}

/// The toolbar on the KDE Header colour: the hamburger menu and Refresh.
fn toolbar(state: &State) -> Element<'_, Message> {
    let t = tokens();
    let refresh = widgets::toolbar_button(
        row![
            icon(Icon::RefreshCw, t.font_size),
            label("Refresh games").size(t.small_font_size())
        ]
        .spacing(t.rem(2.0))
        .align_y(Alignment::Center),
        t.rem(3.0),
        (!state.loading).then_some(Message::Refresh),
    );

    container(
        row![crate::menu::main_menu(), refresh]
            .spacing(t.rem(1.0))
            .align_y(Alignment::Center),
    )
    .padding([t.rem(1.0), t.rem(2.0)])
    .width(Length::Fill)
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(t.header)),
        ..container::Style::default()
    })
    .into()
}

/// The table pane above the log pane, split by an invisible handle.
fn split(state: &State) -> Element<'_, Message> {
    let grid = pane_grid(&state.panes, |_pane, kind, _maximized| {
        pane_grid::Content::new(match kind {
            PaneKind::Table => table_pane(state),
            PaneKind::Log => logs_ui::view(&state.log),
        })
    })
    .spacing(SPLIT_GAP)
    .on_resize(6, Message::PaneResized)
    .style(|_theme: &Theme| {
        let invisible = pane_grid::Line {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
        };
        pane_grid::Style {
            hovered_region: pane_grid::Highlight {
                background: Background::Color(iced::Color::TRANSPARENT),
                border: iced::Border::default(),
            },
            picked_split: invisible,
            hovered_split: invisible,
        }
    });

    sensor(grid)
        .on_show(Message::SplitResized)
        .on_resize(Message::SplitResized)
        .into()
}

/// The table above the action row.
fn table_pane(state: &State) -> Element<'_, Message> {
    let t = tokens();
    column![table::view(state), action_row(state)]
        .spacing(t.rem(1.0))
        .padding(iced::Padding {
            bottom: t.rem(1.0),
            ..iced::Padding::ZERO
        })
        .into()
}

/// Browse plus the four Wine tools; all need a selected game.
fn action_row(state: &State) -> Element<'_, Message> {
    let enabled = state.selected.is_some();
    let action = |text: &'static str| {
        widgets::outline_button(text, None, enabled.then_some(Message::MenuOpened), false).into()
    };
    let mut buttons: Vec<Element<'_, Message>> = vec![action("Browse...")];
    buttons.extend(TOOL_BUTTONS.iter().map(|(name, _tool)| action(name)));
    row(buttons).spacing(tokens().rem(1.0)).into()
}

/// The spinner while a launch runs, the status text, and the selected App
/// ID on the right.
fn status_bar(state: &State) -> Element<'_, Message> {
    let selected = state
        .selected_row()
        .map(|row| format!("Selected: {}", row.app_id))
        .unwrap_or_default();

    let t = tokens();
    let mut left = row![].spacing(t.rem(2.0)).align_y(Alignment::Center);
    if state.launch_running {
        left = left.push(label("…"));
    }
    left = left.push(label(state.status_text.as_str()));

    let bar = container(
        row![left, Space::new().width(Length::Fill), label(selected)].align_y(Alignment::Center),
    )
    .height(STATUS_BAR_HEIGHT)
    .padding([0.0, t.rem(2.0)])
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let rule = container(Space::new().width(Length::Fill).height(1)).style(move |_theme: &Theme| {
        container::Style {
            background: Some(Background::Color(t.border)),
            ..container::Style::default()
        }
    });
    column![rule, bar].into()
}
