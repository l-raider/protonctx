//! Launch executables inside a game's Proton prefix.

pub mod proton;

use crate::models::Game;

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

impl std::error::Error for LaunchError {}

/// Launch a built-in Wine tool (e.g. `winecfg`) in the game's prefix.
pub fn launch_tool(game: &Game, tool: &str) -> Result<(), LaunchError> {
    proton::run_in_prefix(game, &[tool])
}
