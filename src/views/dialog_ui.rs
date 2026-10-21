//! Dialog windows: their kinds, window settings, and the shared chrome.
//!
//! Every dialog (About, Settings, Launch Error, Delete Shader Cache) is its
//! own OS window, one per kind, registered in `State::dialogs`. The chrome is
//! shared: padding, a vertically scrolling body, and a right-aligned footer
//! of outline buttons whose default carries Qt's focus border. Enter/Space
//! activate the default button and Esc (or the WM close button) dismisses,
//! unless the dialog is locked while it works.

use std::path::PathBuf;

use iced::widget::{Space, column, container, row, scrollable, text};
use iced::{Alignment, Background, Element, Length, Point, Size, Theme, window};

use crate::app::{Message, State};
use crate::assets::Icon;
use crate::games::GameRow;
use crate::theme::tokens;
use crate::views::widgets::{self, icon, label};
use crate::views::{about_ui, settings_ui};

/// Size of the message-box badges (the error circle and warning triangle).
pub const BADGE_SIZE: f32 = 56.0;
/// Body copy line height, matching Qt's message boxes.
const BODY_LINE_HEIGHT: f32 = 1.25;

/// The kinds of dialog, one window each.
#[derive(Clone, Debug, PartialEq)]
pub enum DialogKind {
    About,
    Settings,
    LaunchError(String),
    DeleteShaderCache {
        /// The game whose cache is deleted, captured when the dialog opened.
        game: GameRow,
        path: PathBuf,
        /// True while the deletion runs: the buttons are disabled and the
        /// dialog cannot be dismissed.
        deleting: bool,
    },
}

/// A dialog's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    Ok,
    Yes,
    No,
}

impl DialogKind {
    /// Kinds that share a window: reopening one replaces the other's content.
    pub fn same_window(&self, other: &DialogKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    pub fn title(&self) -> &'static str {
        match self {
            DialogKind::About => "About protonctx",
            DialogKind::Settings => "Settings",
            DialogKind::LaunchError(_) => "Launch Error",
            DialogKind::DeleteShaderCache { .. } => "Delete Shader Cache",
        }
    }

    /// The window size, which is also its minimum.
    pub fn size(&self) -> Size {
        match self {
            // Wide enough that the description stays on one line at KDE's
            // 10 pt.
            DialogKind::About => Size::new(400.0, 200.0),
            DialogKind::Settings => Size::new(300.0, 100.0),
            // Wide and short, like the Qt message box.
            DialogKind::LaunchError(_) => Size::new(500.0, 130.0),
            // Room for the cache path and the closing paragraph.
            DialogKind::DeleteShaderCache { .. } => Size::new(520.0, 230.0),
        }
    }

    /// Esc and the WM close button are ignored while this is false.
    pub fn dismissible(&self) -> bool {
        !matches!(self, DialogKind::DeleteShaderCache { deleting: true, .. })
    }

    /// The button Enter/Space activates: No for the destructive
    /// confirmation (the safe choice, as in Qt), OK everywhere else.
    pub fn default_button(&self) -> DialogButton {
        match self {
            DialogKind::DeleteShaderCache { .. } => DialogButton::No,
            _ => DialogButton::Ok,
        }
    }

    /// The footer buttons, left to right: `(button, label, icon)`.
    fn buttons(&self) -> &'static [(DialogButton, &'static str, Icon)] {
        match self {
            DialogKind::DeleteShaderCache { .. } => &[
                (DialogButton::Yes, "Yes", Icon::Check),
                (DialogButton::No, "No", Icon::Ban),
            ],
            _ => &[(DialogButton::Ok, "OK", Icon::Check)],
        }
    }
}

/// The confirmation text, matching the legacy dialog.
pub fn delete_cache_message(path: &std::path::Path) -> String {
    format!(
        "This will permanently delete the shader cache directory:\n\n{}\n\n\
         Shader caches are rebuilt automatically the next time the game runs, \
         but for best results close the game before deleting.",
        path.display()
    )
}

/// Window settings for `kind`: fixed size, server-side decorations, the
/// app's desktop-file id, centred over the parent where its position is
/// known (X11). On Wayland the compositor places the window.
pub fn window_settings(kind: &DialogKind, parent: Option<(Point, Size)>) -> window::Settings {
    let size = kind.size();
    let position = match parent {
        Some((origin, parent_size)) => window::Position::Specific(Point::new(
            origin.x + (parent_size.width - size.width) / 2.0,
            origin.y + (parent_size.height - size.height) / 2.0,
        )),
        None => window::Position::Centered,
    };
    window::Settings {
        size,
        min_size: Some(size),
        resizable: false,
        minimizable: false,
        position,
        decorations: true,
        icon: crate::assets::window_icon(),
        // The WM close button is routed through the same dismiss rule as Esc.
        exit_on_close_request: false,
        platform_specific: window::settings::PlatformSpecific {
            application_id: crate::flatpak::window_app_id(),
            ..Default::default()
        },
        ..window::Settings::default()
    }
}

/// Body copy at the theme size with Qt's 1.25 line height.
pub fn body_text<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    label(content).line_height(BODY_LINE_HEIGHT)
}

/// The dialog window's view: body above the footer, inside the chrome.
pub fn view<'a>(id: window::Id, kind: &'a DialogKind, state: &'a State) -> Element<'a, Message> {
    let t = tokens();
    let body: Element<'a, Message> = match kind {
        DialogKind::About => about_ui::body(),
        DialogKind::Settings => settings_ui::body(state.remember_last_dir),
        DialogKind::LaunchError(message) => error_body(message),
        DialogKind::DeleteShaderCache { path, .. } => warning_body(delete_cache_message(path)),
    };

    let locked = !kind.dismissible();
    let default = kind.default_button();
    let mut footer = row![Space::new().width(Length::Fill)]
        .spacing(t.rem(2.0))
        .align_y(Alignment::Center);
    for &(button, text_value, leading) in kind.buttons() {
        let on_press = (!locked).then_some(Message::DialogButton(id, button));
        footer = footer.push(widgets::outline_button(
            text_value,
            Some(leading),
            on_press,
            button == default,
        ));
    }

    let content = column![
        scrollable(container(body).width(Length::Fill))
            .direction(scrollable::Direction::Vertical(widgets::scrollbar()))
            .height(Length::Fill)
            .style(widgets::scrollable_style),
        footer,
    ]
    .spacing(t.rem(2.0));

    container(content)
        .padding(t.rem(4.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.background)),
            text_color: Some(t.foreground),
            ..container::Style::default()
        })
        .into()
}

/// The Launch Error body: Qt's filled danger circle with a white cross, the
/// wrapping message top-aligned beside it.
fn error_body(message: &str) -> Element<'_, Message> {
    let t = tokens();
    let badge = container(icon(Icon::X, 32.0).color(iced::Color::WHITE))
        .width(BADGE_SIZE)
        .height(BADGE_SIZE)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.danger)),
            border: iced::border::rounded(BADGE_SIZE / 2.0),
            ..container::Style::default()
        });
    row![badge, body_text(message).width(Length::Fill)]
        .spacing(t.rem(5.0))
        .align_y(Alignment::Start)
        .into()
}

/// The destructive-confirmation body: Qt's warning triangle beside the
/// message.
fn warning_body<'a>(message: String) -> Element<'a, Message> {
    let t = tokens();
    row![
        icon(Icon::TriangleAlert, BADGE_SIZE).color(t.warning),
        body_text(message).width(Length::Fill),
    ]
    .spacing(t.rem(3.0))
    .align_y(Alignment::Start)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{DialogButton, DialogKind, window_settings};
    use iced::{Point, Size, window};

    #[test]
    fn dialog_sizes_titles_and_defaults() {
        assert_eq!(DialogKind::About.size(), Size::new(400.0, 200.0));
        assert_eq!(DialogKind::Settings.size(), Size::new(300.0, 100.0));
        assert_eq!(
            DialogKind::LaunchError(String::new()).size(),
            Size::new(500.0, 130.0)
        );
        let delete = DialogKind::DeleteShaderCache {
            game: crate::games::GameRow::from_game(&crate::test_support::game(
                "A", 1, "", "/l", "",
            )),
            path: "/l/steamapps/shadercache/1".into(),
            deleting: false,
        };
        assert_eq!(delete.size(), Size::new(520.0, 230.0));
        assert_eq!(delete.default_button(), DialogButton::No);
        assert_eq!(DialogKind::About.default_button(), DialogButton::Ok);
        assert!(delete.dismissible());
        assert_eq!(delete.title(), "Delete Shader Cache");
        assert!(
            DialogKind::LaunchError("a".into()).same_window(&DialogKind::LaunchError("b".into()))
        );
        assert!(!DialogKind::About.same_window(&DialogKind::Settings));
    }

    #[test]
    fn dialogs_centre_on_a_known_parent_and_fix_their_size() {
        let settings = window_settings(
            &DialogKind::About,
            Some((Point::new(100.0, 100.0), Size::new(760.0, 520.0))),
        );
        assert!(matches!(
            settings.position,
            window::Position::Specific(Point { x: 280.0, y: 260.0 })
        ));
        assert_eq!(settings.min_size, Some(settings.size));
        assert!(!settings.resizable);
        assert!(!settings.exit_on_close_request);

        let unknown = window_settings(&DialogKind::About, None);
        assert!(matches!(unknown.position, window::Position::Centered));
    }
}
