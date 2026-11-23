//! Locate the Steam installation root.

use std::path::{Path, PathBuf};

use crate::xdg;

use super::paths;

/// Candidate Steam roots, in priority order. The first that contains the data files we
/// need wins. These mirror the locations Steam uses on Linux; `~/.steam/root`,
/// `~/.steam/steam` and `~/.steam/debian-installation` are symlinks that all resolve to
/// `~/.local/share/Steam` on a normal installation, but we check them all to be safe.
const CANDIDATES: [&str; 4] = [
    ".local/share/Steam",
    ".steam/root",
    ".steam/steam",
    ".steam/debian-installation",
];

/// Find the Steam root directory by checking known locations for the presence of the
/// `steamapps` data directory.
pub fn find_steam_root() -> Option<PathBuf> {
    find_steam_root_in(&xdg::home_dir()?)
}

/// [`find_steam_root`] relative to an explicit home directory.
fn find_steam_root_in(home: &Path) -> Option<PathBuf> {
    CANDIDATES
        .iter()
        .map(|c| home.join(c))
        .find(|p| paths::steamapps_dir(p).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{is_child, run_in_child, temp_dir};

    #[test]
    fn candidates_are_relative_to_home() {
        assert_eq!(Path::new(CANDIDATES[0]), Path::new(".local/share/Steam"));
    }

    #[test]
    fn first_candidate_with_steamapps_wins() {
        let home = temp_dir("locations_priority");
        for candidate in [".steam/steam", ".steam/debian-installation"] {
            std::fs::create_dir_all(home.join(candidate).join("steamapps")).unwrap();
        }
        // A candidate without `steamapps/` is not a Steam root.
        std::fs::create_dir_all(home.join(".local/share/Steam")).unwrap();

        assert_eq!(find_steam_root_in(&home), Some(home.join(".steam/steam")));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn none_when_no_candidate_has_steamapps() {
        let home = temp_dir("locations_none");
        std::fs::create_dir_all(&home).unwrap();

        assert_eq!(find_steam_root_in(&home), None);

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn steam_root_is_found_under_home() {
        let home = std::env::temp_dir().join("protonctx_test_locations_home");
        if !is_child() {
            std::fs::create_dir_all(home.join(".local/share/Steam/steamapps")).unwrap();
            run_in_child(
                "steam::locations::tests::steam_root_is_found_under_home",
                &[("HOME", home.as_os_str())],
                &[],
            );
            std::fs::remove_dir_all(&home).ok();
            return;
        }
        assert_eq!(find_steam_root(), Some(home.join(".local/share/Steam")));
    }

    #[test]
    fn no_steam_root_without_home() {
        if !is_child() {
            return run_in_child(
                "steam::locations::tests::no_steam_root_without_home",
                &[],
                &["HOME"],
            );
        }
        assert_eq!(find_steam_root(), None);
    }
}
