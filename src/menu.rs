//! Popup menus: the toolbar's hamburger menu (an `iced_aw` menu bar with one
//! root) and the games table's row context menu (an `iced_aw` context menu
//! overlay). Both share one item recipe, so their padding, font size and
//! hover tint cannot drift apart.

use iced::widget::{Space, button, column, container, row};
use iced::{Alignment, Background, Border, Element, Length, Shadow, Theme};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};

use crate::app::Message;
use crate::assets::Icon;
use crate::games::TOOL_BUTTONS;
use crate::theme::tokens;
use crate::views::widgets::{self, faded, icon, label};

/// Height of one menu item.
const ITEM_HEIGHT: f32 = 28.0;
/// Padding inside the menu panel, and its distance from the hamburger.
const MENU_PADDING: f32 = 4.0;
const MENU_OFFSET: f32 = 2.0;
/// Menu panel widths, matching the gpui popups.
const MAIN_MENU_WIDTH: f32 = 140.0;
const CONTEXT_MENU_WIDTH: f32 = 200.0;

/// One menu item: an optional leading icon and the label at the theme's base
/// font size (not a smaller menu size). `None` disables it.
fn item<'a>(
    text: &'a str,
    leading: Option<Icon>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let t = tokens();
    let content: Element<'a, Message> = match leading {
        Some(leading) => row![icon(leading, t.font_size - 2.0), label(text)]
            .spacing(t.rem(1.0))
            .align_y(Alignment::Center)
            .into(),
        None => label(text).into(),
    };
    let enabled = on_press.is_some();
    widgets::arrow_cursor(
        button(
            container(content)
                .height(Length::Fill)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(ITEM_HEIGHT)
        .padding([0, 8])
        .on_press_maybe(on_press)
        .style(move |_theme: &Theme, status| {
            item_style(widgets::effective_status(status, enabled))
        }),
    )
}

/// Menu items are flat; hovering tints them with the selection at 0.3.
fn item_style(status: button::Status) -> button::Style {
    let t = tokens();
    let base = button::Style {
        background: None,
        text_color: t.menu_foreground,
        border: Border {
            radius: t.radius.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: true,
    };
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(t.accent)),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: faded(t.menu_foreground, t.disabled_opacity),
            ..base
        },
    }
}

/// A horizontal separator line with breathing room above and below.
fn separator<'a>() -> Element<'a, Message> {
    let t = tokens();
    container(
        container(Space::new().width(Length::Fill).height(1)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(Background::Color(t.border)),
                ..container::Style::default()
            }
        }),
    )
    .padding([4, 0])
    .width(Length::Fill)
    .into()
}

/// The popup panel: the window fill (KDE draws menus on it) and the frame
/// colour, rounded. No drop shadow: iced_tiny_skia's damage tracking only
/// repaints a quad's own bounds, so a shadow outside them smears and
/// flickers as the menu repaints.
fn panel_border() -> Border {
    let t = tokens();
    Border {
        color: t.border,
        width: 1.0,
        radius: t.radius.into(),
    }
}

/// The hamburger button with its menu: Settings, Clear logs, About, and
/// Exit.
pub fn main_menu<'a>() -> Element<'a, Message> {
    let t = tokens();
    let items = vec![
        Item::new(item(
            "Settings",
            Some(Icon::Settings),
            Some(Message::OpenSettings),
        )),
        Item::new(item(
            "Clear logs",
            Some(Icon::Eraser),
            Some(Message::ClearLogs),
        )),
        Item::new(item(
            "About protonctx",
            Some(Icon::Info),
            Some(Message::OpenAbout),
        )),
        Item::new(separator()),
        Item::new(item("Exit", Some(Icon::LogOut), Some(Message::Exit))),
    ];
    let menu = Menu::new(items)
        .width(MAIN_MENU_WIDTH)
        .padding(MENU_PADDING)
        .offset(MENU_OFFSET);

    let root = widgets::toolbar_button(
        icon(Icon::Menu, t.font_size),
        0.0,
        Some(Message::MenuOpened),
    );

    MenuBar::new(vec![Item::with_menu(root, menu)])
        .draw_path(DrawPath::FakeHovering)
        // iced_aw leaves both off by default: picking an item or clicking
        // outside must close the menu, as in Qt.
        .close_on_item_click_global(true)
        .close_on_background_click_global(true)
        .padding(0)
        .spacing(0)
        .style(menu_bar_style)
        .into()
}

/// `native-theme`'s menu style with the Breeze overrides: the panel on the
/// window fill, the bar itself invisible inside the toolbar.
fn menu_bar_style(
    theme: &Theme,
    status: iced_aw::style::Status,
) -> iced_aw::style::menu_bar::Style {
    let t = tokens();
    let native = native_theme_iced::styles::aw::menu(&t.resolved)(theme, status);
    iced_aw::style::menu_bar::Style {
        bar_background: Background::Color(iced::Color::TRANSPARENT),
        bar_border: Border::default(),
        bar_shadow: Shadow::default(),
        menu_background: Background::Color(t.menu_background),
        menu_border: panel_border(),
        menu_shadow: Shadow::default(),
        path: Background::Color(t.accent),
        ..native
    }
}

/// The games table's row context menu. Delete Shader Cache is disabled
/// unless the selected game has a cache directory.
pub fn row_context_menu<'a>(cache_exists: bool) -> Element<'a, Message> {
    let t = tokens();
    let mut entries: Vec<Element<'a, Message>> = vec![
        item("Browse for executable...", None, Some(Message::Browse)),
        separator(),
    ];
    for (name, tool) in TOOL_BUTTONS {
        entries.push(item(name, None, Some(Message::LaunchTool(tool))));
    }
    entries.push(separator());
    entries.push(item(
        "Copy compatdata path",
        None,
        Some(Message::CopyCompatDataPath),
    ));
    entries.push(item(
        "Copy compatibility tool path",
        None,
        Some(Message::CopyToolPath),
    ));
    entries.push(separator());
    entries.push(item(
        "Delete Shader Cache",
        None,
        cache_exists.then_some(Message::ConfirmDeleteShaderCache),
    ));

    container(column(entries))
        .width(CONTEXT_MENU_WIDTH)
        .padding(MENU_PADDING)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.menu_background)),
            border: panel_border(),
            ..container::Style::default()
        })
        .into()
}
