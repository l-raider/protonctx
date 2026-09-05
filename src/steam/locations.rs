//! Locate the Steam installation root.

use std::path::PathBuf;

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

/// Return the home directory, if it can be determined.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Find the Steam root directory by checking known locations for the presence of the
/// `steamapps` data directory.
pub fn find_steam_root() -> Option<PathBuf> {
    let home = home_dir()?;

    CANDIDATES
        .iter()
        .map(|c| home.join(c))
        .find(|p| p.join("steamapps").is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn candidates_are_relative_to_home() {
        let home = home_dir().unwrap();
        let first = home.join(".local/share/Steam");
        assert_eq!(Path::new(CANDIDATES[0]), Path::new(".local/share/Steam"));
        let _ = first;
    }
}
