//! Shared data structures for protonctx.
//!
//! `Game` is the display + launch record shown in the Qt games table and passed to the
//! Proton launcher. It is kept deliberately small: the UI only needs display fields plus
//! the paths used to launch executables (`library_path`, `steam_root`, `proton_dir`).

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
    /// `"/home/username/.local/share/Steam"`.
    pub library_path: String,
    /// Absolute path to the Steam client install the game was discovered through (the
    /// Steam root), exported to Proton as `STEAM_COMPAT_CLIENT_INSTALL_PATH`. This is
    /// not `library_path`: a game on a secondary library still runs with the root's
    /// client files.
    pub steam_root: String,
    /// Absolute path to the Proton compatibility tool directory that this game runs with,
    /// e.g. `"/home/username/.local/share/Steam/steamapps/common/Proton - Experimental"`.
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

#[cfg(test)]
mod tests {
    use crate::test_support::{game, temp_dir};

    #[test]
    fn proton_script_is_none_without_a_proton_dir() {
        let game = game("No Prefix", 1, "", "/lib", "");
        assert_eq!(game.proton_script(), None);
    }

    #[test]
    fn proton_script_is_the_proton_file_when_it_exists() {
        let dir = temp_dir("models_proton_script");
        std::fs::create_dir_all(&dir).unwrap();
        let game = game("Has Proton", 2, "proton_9", "/lib", dir.to_str().unwrap());

        // The directory is resolved but holds no `proton` script yet.
        assert_eq!(game.proton_script(), None);

        std::fs::write(dir.join("proton"), "").unwrap();
        assert_eq!(game.proton_script(), Some(dir.join("proton")));

        std::fs::remove_dir_all(&dir).ok();
    }
}
