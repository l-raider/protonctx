//! Test-only helpers shared across the crate's unit tests.
//!
//! [`open_app`] assumes `gpui_kit::init` has already run: every test calls
//! `cx.update(gpui_kit::init)` itself and some install a theme preset before
//! opening, so initialising here would double-init the app-global state.

use gpui_kit::component::Root;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};

use crate::models::Game;
use crate::views::main_ui::ProtonctxApp;

/// A fresh temp directory named for `tag` and this process.
///
/// Each test uses a unique `tag`, so parallel tests never share a directory.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("protonctx_test_{tag}_{}", std::process::id()))
}

/// Open the main window with a fresh [`ProtonctxApp`].
///
/// `gpui_kit::init` must have run in `cx` before calling this.
pub fn open_app(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ProtonctxApp>) {
    let mut view = None;
    let handle = cx.open_window(size(px(760.), px(520.)), |window, cx| {
        let app = cx.new(|cx| ProtonctxApp::new(window, cx));
        view = Some(app.clone());
        Root::new(app, window, cx)
    });
    (handle.into(), view.expect("open_app creates ProtonctxApp"))
}

/// A [`Game`] fixture with every field populated.
pub fn game(
    name: &str,
    app_id: u32,
    compat_tool: &str,
    library_path: &str,
    proton_dir: &str,
) -> Game {
    Game {
        name: name.to_string(),
        app_id,
        compat_tool: compat_tool.to_string(),
        library_path: library_path.to_string(),
        proton_dir: proton_dir.to_string(),
    }
}
