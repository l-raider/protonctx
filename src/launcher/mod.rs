//! Launch executables inside a game's Proton prefix.

pub mod proton;

use std::path::{Path, PathBuf};

use crate::models::Game;

/// A spawned Proton launch: the live child process plus the human-readable command
/// line used to start it (built from full paths so the UI log can show exactly what
/// was executed).
pub struct LaunchedProcess {
    pub child: std::process::Child,
    pub command_line: String,
}

/// What kind of command we're launching into a prefix.
#[derive(Debug)]
pub enum LaunchError {
    /// The game's Proton directory could not be resolved (no prefix yet).
    NoProtonDir,
    /// The process failed to spawn; carries the resolved script path and the
    /// underlying I/O error for an actionable message.
    Spawn {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NoProtonDir => {
                write!(
                    f,
                    "no Proton directory found for this game (has it been run yet?)"
                )
            }
            LaunchError::Spawn { path, source } => {
                write!(f, "failed to start process `{}`: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LaunchError::NoProtonDir => None,
            LaunchError::Spawn { source, .. } => Some(source),
        }
    }
}

/// Launch a built-in Wine tool (e.g. `winecfg`) in the game's prefix.
///
/// Returns the spawned [`LaunchedProcess`] so the caller can track its lifetime
/// and log its output. Because `proton runinprefix` blocks until the target
/// executable exits (it calls `subprocess.call`), waiting on the child is
/// equivalent to waiting for the launched tool to finish.
pub fn launch_tool(game: &Game, tool: &str) -> Result<LaunchedProcess, LaunchError> {
    proton::run_in_prefix(game, &[tool], None)
}

/// Run an executable the user picked in `game`'s prefix, from the executable's own
/// folder.
///
/// Windows launchers (Explorer, Steam) start a program in its own folder, and many
/// trainers, mod managers and installers open their files by relative path, so the
/// launch does the same. When the folder cannot be seen the launch still goes ahead,
/// from protonctx's own working directory.
///
/// Inside a Flatpak sandbox `exe` may be a document-portal alias; it is resolved to
/// its host path first (see [`crate::flatpak::resolve_host_path`]), since the command
/// runs on the host.
pub fn run_executable(game: &Game, exe: &Path) -> Result<LaunchedProcess, LaunchError> {
    let host_exe = if crate::flatpak::running_in_flatpak() {
        crate::flatpak::resolve_host_path(&exe.to_string_lossy())
            .map_or_else(|| exe.to_path_buf(), PathBuf::from)
    } else {
        exe.to_path_buf()
    };
    let cwd = working_dir(exe, &host_exe);
    proton::run_in_prefix(game, &[&host_exe.to_string_lossy()], cwd)
}

/// The working directory for a picked executable: the folder of `host_exe` (the path
/// the host sees), provided the folder of `picked` (the path this process sees)
/// exists. Inside a sandbox the host path itself may not be visible, so the check is
/// made on the picked one.
fn working_dir<'a>(picked: &Path, host_exe: &'a Path) -> Option<&'a Path> {
    picked.parent().filter(|dir| dir.is_dir())?;
    host_exe.parent().filter(|dir| !dir.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_dir_is_the_host_folder_when_the_picked_folder_exists() {
        let dir = crate::test_support::temp_dir("working_dir");
        std::fs::create_dir_all(&dir).unwrap();
        let picked = dir.join("trainer.exe");

        // Outside a sandbox both paths are the same.
        assert_eq!(working_dir(&picked, &picked), Some(dir.as_path()));
        // A resolved document-portal pick runs from the real host folder.
        let host = Path::new("/mnt/games/Trainer/trainer.exe");
        assert_eq!(
            working_dir(&picked, host),
            Some(Path::new("/mnt/games/Trainer"))
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn working_dir_is_unset_when_the_folder_is_missing() {
        let missing = crate::test_support::temp_dir("working_dir_missing").join("t.exe");
        assert_eq!(working_dir(&missing, &missing), None);
        // A bare file name has no folder to start in.
        assert_eq!(working_dir(Path::new("t.exe"), Path::new("t.exe")), None);
    }
}
