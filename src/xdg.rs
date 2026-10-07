//! Shared HOME and XDG base-directory resolution (config, Steam discovery,
//! Flatpak integration, launch-root fallback).

use std::path::PathBuf;

/// Return the home directory, if it can be determined.
///
/// `$HOME` is the single source of truth for every module that needs it, so the
/// precedence is identical across config, Steam discovery, Flatpak integration,
/// and the launcher.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolve an XDG base directory for the given environment variable, falling
/// back to the given `$HOME`-relative suffix when the variable is unset, empty,
/// or relative.
///
/// XDG base directories must be absolute paths per the spec; a relative value is
/// ignored in favour of the default.
pub fn xdg_dir(env_var: &str, home_fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(env_var) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() && !dir.as_os_str().is_empty() {
            return Some(dir);
        }
    }
    home_dir().map(|home| home.join(home_fallback))
}
