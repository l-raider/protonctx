//! Launch executables inside a game's Proton prefix.

pub mod proton;

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
    proton::run_in_prefix(game, &[tool])
}
