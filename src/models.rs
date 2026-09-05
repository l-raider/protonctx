//! Shared data structures for protonctx.
//!
//! `Game` is the display + launch record shown in the Qt games table and passed to the
//! Proton launcher. It is kept deliberately small: the UI only needs display fields plus
//! the `proton_dir` used to launch executables.

/// A single installed Steam game (or Steam "app") that can be run under Proton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// Human-readable name, e.g. `"Broforce"`.
    pub name: String,
    /// Steam App ID, e.g. `274190`.
    pub app_id: u32,
    /// The compatibility tool selected for this game (Steam's internal name, e.g.
    /// `"proton_experimental"`), or the empty string when Steam has no explicit mapping
    /// and falls back to the default. Shown verbatim in the UI.
    pub compat_tool: String,
    /// Absolute path to the Steam library this game is installed in, e.g.
    /// `"/home/lraider/.local/share/Steam"`.
    pub library_path: String,
    /// Absolute path to the Proton compatibility tool directory that this game runs with,
    /// e.g. `"/home/lraider/.local/share/Steam/steamapps/common/Proton - Experimental"`.
    /// Empty when it could not be resolved.
    ///
    /// Prefer resolving this *from the currently-selected `compat_tool`* (see
    /// `steam::compat::proton_dir_for_tool`): a prefix's `config_info` records the tool
    /// that *created* the prefix and goes stale when the user switches tools in Steam
    /// without recreating the prefix, so it is only a fallback. If the selected tool
    /// cannot be located, the launcher and the "copy tool path" UI fall back to the
    /// prefix's recorded tool.
    pub proton_dir: String,
}

impl Game {
    /// The `proton` launcher script inside `proton_dir`, if the directory was resolved.
    pub fn proton_script(&self) -> Option<std::path::PathBuf> {
        if self.proton_dir.is_empty() {
            return None;
        }
        let p = std::path::Path::new(&self.proton_dir).join("proton");
        p.is_file().then_some(p)
    }
}
