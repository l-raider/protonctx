//! Invoke the `proton` launcher script to run an executable inside a prefix.
//!
//! This mirrors what the reference `proton-exec.sh` / `run-exe.sh` scripts do: set the
//! Steam compat environment variables and call `<proton_dir>/proton runinprefix <args>`.
//! The `runinprefix` verb runs the given command via the prefix's Wine, which is what we
//! want for arbitrary `.exe` files and Wine built-ins (winecfg, taskmgr, ...) alike.

use std::process::{Command, Stdio};

use crate::models::Game;

use super::{LaunchError, LaunchedProcess};

/// Run an arbitrary command (`.exe` path or Wine built-in) inside `game`'s Proton prefix.
///
/// `args` are the positional arguments passed after the `runinprefix` verb. This spawns
/// the process in the background (non-blocking), matching how a GUI launcher should behave.
///
/// stdout/stderr are piped so the caller can stream their contents to the UI log; stdin is
/// left inherited so interactive executables that read from the terminal still work.
///
/// Returns the spawned [`LaunchedProcess`] so the caller can observe when the process
/// finishes and log its output. Note that `proton runinprefix` itself blocks until the
/// target executable exits (it invokes `subprocess.call`), so waiting on the returned child
/// tracks the lifetime of the launched executable, not just the wrapper script.
pub fn run_in_prefix(game: &Game, args: &[&str]) -> Result<LaunchedProcess, LaunchError> {
    let proton = game.proton_script().ok_or(LaunchError::NoProtonDir)?;

    let root = steam_root_for(game);
    // Proton prefixes (compatdata) live under the library the game is installed in,
    // NOT the Steam root: a game on a secondary library keeps its prefix there.
    let compat_data = compat_data_dir_for(std::path::Path::new(&game.library_path), game.app_id);

    let mut cmd = Command::new(&proton);
    cmd.arg("runinprefix");
    cmd.args(args);

    cmd.env("STEAM_COMPAT_DATA_PATH", &compat_data);
    cmd.env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &root);
    cmd.env("SteamGameId", game.app_id.to_string());
    cmd.env("SteamAppId", game.app_id.to_string());

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(child) => {
            // Human-readable command line using full paths, so the log shows exactly
            // what was launched: the proton script, the `runinprefix` verb, the
            // arguments, and the compatdata (prefix) directory it targets.
            let command_line = format!(
                "{} runinprefix {} (prefix: {})",
                proton.display(),
                args.join(" "),
                compat_data.display()
            );
            Ok(LaunchedProcess {
                child,
                command_line,
            })
        }
        Err(source) => Err(LaunchError::Spawn {
            path: proton,
            source,
        }),
    }
}

/// Resolve the Steam installation root for a game.
///
/// The game's `library_path` may be a secondary library (e.g. on another disk), so we
/// cannot assume it equals the Steam root. The robust approach is to derive the root from
/// the Proton script path: built-in tools live at `<steam_root>/steamapps/common/<name>/proton`,
/// and custom tools at `<steam_root>/compatibilitytools.d/<name>/proton` — both share the
/// same `<steam_root>/steamapps` or `<steam_root>/compatibilitytools.d` ancestor.
///
/// The root is used both for `STEAM_COMPAT_CLIENT_INSTALL_PATH` and to locate
/// `steamapps/compatdata/<appid>` (see [`run_in_prefix`]).
///
/// As a pragmatic fallback (when derivation fails), the environment's `HOME`-based default
/// `~/.local/share/Steam` is used, then finally `game.library_path`.
///
/// This is the single source of truth for Steam-root resolution, shared with
/// `AppBackend::compat_data_path()` so the "Copy compatdata path" UI always matches the
/// path a launch actually uses.
pub fn steam_root_for(game: &Game) -> std::path::PathBuf {
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

/// The compatdata (prefix) directory for an app, under the library it is installed in.
///
/// Proton prefixes live under the *library*'s `steamapps/compatdata/` (Steam creates the
/// prefix next to the game install, so a game on a secondary library keeps its prefix
/// there, not under the Steam root).
pub fn compat_data_dir_for(library: &std::path::Path, app_id: u32) -> std::path::PathBuf {
    library
        .join("steamapps")
        .join("compatdata")
        .join(app_id.to_string())
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
