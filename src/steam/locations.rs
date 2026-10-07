//! Locate the Steam installation root.

use std::path::PathBuf;

use crate::xdg;

use super::paths;

/// The default Steam root relative to `$HOME`. Shared with the launcher's
/// home-based fallback so the two cannot drift.
pub const DEFAULT_ROOT: &str = ".local/share/Steam";

/// Candidate Steam roots, in priority order. The first that contains the data files we
/// need wins. These mirror the locations Steam uses on Linux; `~/.steam/root`,
/// `~/.steam/steam` and `~/.steam/debian-installation` are symlinks that all resolve to
/// `~/.local/share/Steam` on a normal installation, but we check them all to be safe.
const CANDIDATES: [&str; 4] = [
    DEFAULT_ROOT,
    ".steam/root",
    ".steam/steam",
    ".steam/debian-installation",
];

/// Find the Steam root directory by checking known locations for the presence of the
/// `steamapps` data directory.
pub fn find_steam_root() -> Option<PathBuf> {
    let home = xdg::home_dir()?;

    CANDIDATES
        .iter()
        .map(|c| home.join(c))
        .find(|p| paths::steamapps_dir(p).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn candidates_are_relative_to_home() {
        let home = xdg::home_dir().unwrap();
        let first = home.join(".local/share/Steam");
        assert_eq!(Path::new(CANDIDATES[0]), Path::new(".local/share/Steam"));
        let _ = first;
    }
}
