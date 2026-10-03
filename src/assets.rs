use std::borrow::Cow;

use gpui_kit::assets::{Assets as ComponentAssets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(ExtraIcons, [Gamepad2, LogOut, Eraser, X]);

/// The packaged application icon, embedded from the packaging tree so the About
/// dialog and the installers share one source of truth.
const APP_ICON_PATH: &str = "icons/protonctx.png";

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON_PATH {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../packaging/icons/hicolor/256x256/apps/protonctx.png"
            ))));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        ComponentAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentAssets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        if APP_ICON_PATH.starts_with(path) {
            paths.push(APP_ICON_PATH.into());
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_extra_icons_and_default_component_icons() {
        for path in [
            "icons/gamepad-2.svg",
            "icons/log-out.svg",
            "icons/eraser.svg",
            "icons/x.svg",
            "icons/protonctx.png",
            "icons/menu.svg",
            "icons/refresh-cw.svg",
        ] {
            let bytes = AppAssets
                .load(path)
                .unwrap_or_else(|error| panic!("{path}: {error}"))
                .unwrap_or_else(|| panic!("{path}: missing"));
            assert!(!bytes.is_empty(), "{path}: empty");
        }
    }

    #[test]
    fn lists_extra_icons_merged_and_deduplicated() {
        let listed = AppAssets.list("icons/").unwrap();
        for path in [
            "icons/gamepad-2.svg",
            "icons/log-out.svg",
            "icons/eraser.svg",
            "icons/x.svg",
            "icons/protonctx.png",
            "icons/menu.svg",
        ] {
            assert!(
                listed.iter().any(|entry| entry == path),
                "{path}: not listed"
            );
        }
        let mut sorted = listed.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(listed, sorted);
    }
}
