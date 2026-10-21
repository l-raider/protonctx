//! Embedded images: the packaged application icon and the Lucide icons the UI
//! draws.
//!
//! Most icons come from the Lucide set bundled with `native-theme`. The few
//! that set does not ship (eraser, log-out, and the dimmed-chevron sort
//! indicators) are embedded from `assets/icons/` (Lucide, ISC licence, see
//! `assets/icons/LICENSE-LUCIDE`).

use std::collections::HashMap;
use std::sync::LazyLock;

use iced::advanced::svg::Handle as SvgHandle;
use iced::widget::image;
use native_theme::theme::{IconData, IconSet};

/// The packaged application icon, embedded from the packaging tree so the
/// About dialog, the window icon, and the installers share one source.
pub const APP_ICON_PNG: &[u8] = include_bytes!("../packaging/icons/hicolor/256x256/apps/protonctx.png");

/// Every icon the UI draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Menu,
    RefreshCw,
    Settings,
    Eraser,
    Info,
    LogOut,
    Check,
    Ban,
    TriangleAlert,
    X,
    SortAscending,
    SortDescending,
}

impl Icon {
    pub const ALL: [Icon; 12] = [
        Icon::Menu,
        Icon::RefreshCw,
        Icon::Settings,
        Icon::Eraser,
        Icon::Info,
        Icon::LogOut,
        Icon::Check,
        Icon::Ban,
        Icon::TriangleAlert,
        Icon::X,
        Icon::SortAscending,
        Icon::SortDescending,
    ];

    /// The SVG source: a Lucide name in `native-theme`'s bundled set, or the
    /// embedded file.
    fn svg(self) -> Option<std::borrow::Cow<'static, [u8]>> {
        let bundled = |name: &str| match native_theme::icons::load_icon(name, IconSet::Lucide) {
            Some(IconData::Svg(bytes)) => Some(bytes),
            _ => None,
        };
        let embedded = |bytes: &'static [u8]| Some(std::borrow::Cow::Borrowed(bytes));
        match self {
            Icon::Menu => bundled("menu"),
            Icon::RefreshCw => bundled("refresh-cw"),
            Icon::Settings => bundled("settings"),
            Icon::Info => bundled("info"),
            Icon::Check => bundled("check"),
            Icon::Ban => bundled("ban"),
            Icon::TriangleAlert => bundled("triangle-alert"),
            Icon::X => bundled("x"),
            Icon::Eraser => embedded(include_bytes!("../assets/icons/eraser.svg")),
            Icon::LogOut => embedded(include_bytes!("../assets/icons/log-out.svg")),
            Icon::SortAscending => embedded(include_bytes!("../assets/icons/sort-ascending.svg")),
            Icon::SortDescending => {
                embedded(include_bytes!("../assets/icons/sort-descending.svg"))
            }
        }
    }
}

/// Handles are built once: a handle's id is a hash of its bytes, so reusing
/// one also reuses the renderer's rasterization cache.
static SVG_HANDLES: LazyLock<HashMap<Icon, SvgHandle>> = LazyLock::new(|| {
    Icon::ALL
        .into_iter()
        .filter_map(|icon| match icon.svg() {
            Some(bytes) => Some((icon, SvgHandle::from_memory(bytes))),
            None => {
                eprintln!("protonctx: icon {icon:?} is not available");
                None
            }
        })
        .collect()
});

/// The SVG handle for `icon`, or `None` when its source failed to load.
pub fn svg_handle(icon: Icon) -> Option<SvgHandle> {
    SVG_HANDLES.get(&icon).cloned()
}

/// The application icon as an image handle (the About dialog badge).
pub fn app_icon() -> image::Handle {
    static HANDLE: LazyLock<image::Handle> =
        LazyLock::new(|| image::Handle::from_bytes(APP_ICON_PNG));
    HANDLE.clone()
}

/// The application icon for window decorations (used where the compositor
/// does not resolve it from the desktop file).
pub fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_file_data(APP_ICON_PNG, None).ok()
}

#[cfg(test)]
mod tests {
    use super::{Icon, app_icon, svg_handle, window_icon};

    #[test]
    fn every_icon_has_an_svg_source() {
        for icon in Icon::ALL {
            let bytes = icon
                .svg()
                .unwrap_or_else(|| panic!("{icon:?}: missing source"));
            assert!(
                std::str::from_utf8(&bytes).is_ok_and(|svg| svg.contains("<svg")),
                "{icon:?}: not an SVG"
            );
            assert!(svg_handle(icon).is_some(), "{icon:?}: no handle");
        }
    }

    #[test]
    fn app_icon_decodes() {
        let _ = app_icon();
        assert!(window_icon().is_some(), "the packaged PNG did not decode");
    }
}
