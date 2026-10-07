//! The About dialog, opened from the toolbar menu.

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;

use crate::views::dialog_ui::{self, DialogAction, DialogContent};

/// The app icon path registered by [`crate::assets::AppAssets`].
const APP_ICON: &str = "icons/protonctx.png";

/// Body of the About dialog; the template provides the window chrome.
struct AboutContent;

impl DialogContent for AboutContent {
    fn id(&self) -> &'static str {
        "about"
    }

    fn title(&self) -> SharedString {
        "About protonctx".into()
    }

    /// Wide enough that the description stays a single line at the KDE 10 pt
    /// rem used in production.
    fn size(&self) -> Size<Pixels> {
        size(px(400.), px(200.))
    }

    fn body(&mut self, _window: &mut Window, cx: &mut App) -> AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            // Raster rather than SVG: GPUI tints SVGs with one color, so the
            // multicolor icon is rendered from the packaged PNG.
            .child(img(APP_ICON).w(px(64.)).h(px(64.)).flex_shrink_0())
            .child(
                dialog_ui::dialog_text(concat!("protonctx v", env!("CARGO_PKG_VERSION")))
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                dialog_ui::dialog_text("Launch executables inside a Steam game's Proton context.")
                    .w_full()
                    .text_center(),
            )
            .child(
                dialog_ui::dialog_text("Licensed under the GNU GPL v3.")
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element()
    }

    fn actions(&self, _cx: &App) -> Vec<DialogAction> {
        // A neutral OK like Qt's `QMessageBox::about`: focused on open so the
        // standard accent focus border shows, and Enter closes the dialog.
        vec![DialogAction::ok("about-ok")]
    }
}

/// Open the About dialog as a native window. The version comes from
/// `CARGO_PKG_VERSION` (Cargo.toml is the single source of truth).
pub fn open_about_dialog(window: &mut Window, cx: &mut App) {
    let _ = dialog_ui::open_dialog(window, cx, AboutContent);
}
