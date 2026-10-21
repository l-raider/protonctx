//! The About dialog body, opened from the toolbar menu.

use iced::widget::{column, container, image};
use iced::{Alignment, Element, Length};

use crate::app::Message;
use crate::theme::tokens;
use crate::views::dialog_ui::body_text;

/// The app icon above the name and version, the description, and the
/// licence line in the secondary text colour. The version comes from
/// `CARGO_PKG_VERSION` (Cargo.toml is the single source of truth).
pub fn body<'a>() -> Element<'a, Message> {
    let t = tokens();
    // Raster rather than SVG: the multicolour icon is the packaged PNG.
    let badge = image(crate::assets::app_icon()).width(64).height(64);
    let content = column![
        badge,
        body_text(concat!("protonctx v", env!("CARGO_PKG_VERSION"))).font(t.semibold_font),
        body_text("Launch executables inside a Steam game's Proton context."),
        body_text("Licensed under the GNU GPL v3.").color(t.muted_foreground),
    ]
    .spacing(t.rem(2.0))
    .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}
