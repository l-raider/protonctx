//! Invoke the `proton` launcher script to run an executable inside a prefix.
//!
//! This mirrors what the reference `proton-exec.sh` / `run-exe.sh` scripts do: set the
//! Steam compat environment variables and call `<proton_dir>/proton runinprefix <args>`.
//! The `runinprefix` verb runs the given command via the prefix's Wine, which is what we
//! want for arbitrary `.exe` files and Wine built-ins (winecfg, taskmgr, ...) alike.

use std::process::Command;

use crate::models::Game;

use super::LaunchError;

/// Run an arbitrary command (`.exe` path or Wine built-in) inside `game`'s Proton prefix.
///
/// `args` are the positional arguments passed after the `runinprefix` verb. This spawns
/// the process in the background (non-blocking), matching how a GUI launcher should behave.
pub fn run_in_prefix(game: &Game, args: &[&str]) -> Result<(), LaunchError> {
    let proton = game.proton_script().ok_or(LaunchError::NoProtonDir)?;

    let compat_data = std::path::Path::new(&game.library_path)
        .join("steamapps")
        .join("compatdata")
        .join(game.app_id.to_string());

    let mut cmd = Command::new(&proton);
    cmd.arg("runinprefix");
    cmd.args(args);

    cmd.env("STEAM_COMPAT_DATA_PATH", &compat_data);
    cmd.env(
        "STEAM_COMPAT_CLIENT_INSTALL_PATH",
        steam_client_install_path(game),
    );
    cmd.env("SteamGameId", game.app_id.to_string());
    cmd.env("SteamAppId", game.app_id.to_string());

    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(LaunchError::Spawn(e.kind())),
    }
}

/// The `STEAM_COMPAT_CLIENT_INSTALL_PATH` points at the Steam installation root.
///
/// The game's `library_path` may be a secondary library (e.g. on another disk), so we
/// cannot assume it equals the Steam root. The robust approach is to derive the root from
/// the Proton script path: built-in tools live at `<steam_root>/steamapps/common/<name>/proton`,
/// and custom tools at `<steam_root>/compatibilitytools.d/<name>/proton` — both share the
/// same `<steam_root>/steamapps` or `<steam_root>/compatibilitytools.d` ancestor.
///
/// As a pragmatic fallback (when derivation fails), the environment's `HOME`-based default
/// `~/.local/share/Steam` is used.
fn steam_client_install_path(game: &Game) -> std::path::PathBuf {
    if let Some(root) = steam_root_from_proton_dir(&game.proton_dir) {
        return root;
    }

    if let Some(home) = std::env::var_os("HOME") {
        let default = std::path::PathBuf::from(home).join(".local/share/Steam");
        if default.is_dir() {
            return default;
        }
    }

    std::path::PathBuf::from(&game.library_path)
}

/// Derive the Steam root from a Proton directory path.
///
/// - Built-in tools: `<steam_root>/steamapps/common/<name>` → the Steam root is two
///   directories above the `common` marker (i.e. `common`'s grandparent).
/// - Custom tools: `<steam_root>/compatibilitytools.d/<name>` → the Steam root is the
///   `compatibilitytools.d` directory's parent.
fn steam_root_from_proton_dir(proton_dir: &str) -> Option<std::path::PathBuf> {
    let mut path = std::path::Path::new(proton_dir);

    // Walk up to find a directory named `common` or `compatibilitytools.d`.
    while let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name == "common" {
            // common -> steamapps -> steam root
            return path.parent()?.parent().map(|p| p.to_path_buf());
        }
        if name == "compatibilitytools.d" {
            return path.parent().map(|p| p.to_path_buf());
        }
        path = path.parent()?;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_root_from_builtin_proton() {
        let root = steam_root_from_proton_dir(
            "/home/u/.local/share/Steam/steamapps/common/Proton - Experimental",
        );
        assert_eq!(
            root,
            Some(std::path::PathBuf::from("/home/u/.local/share/Steam"))
        );
    }

    #[test]
    fn derives_root_from_custom_proton() {
        let root = steam_root_from_proton_dir(
            "/home/u/.local/share/Steam/compatibilitytools.d/GE-Proton10-34",
        );
        assert_eq!(
            root,
            Some(std::path::PathBuf::from("/home/u/.local/share/Steam"))
        );
    }

    #[test]
    fn none_when_no_marker_dir() {
        assert!(steam_root_from_proton_dir("/some/random/path").is_none());
    }
}
