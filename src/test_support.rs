//! Test-only helpers shared across the crate's unit tests.

use crate::models::Game;

/// A fresh temp directory named for `tag` and this process.
///
/// Each test uses a unique `tag`, so parallel tests never share a directory.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("protonctx_test_{tag}_{}", std::process::id()))
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
