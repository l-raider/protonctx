//! Launch executables inside a game's Proton prefix.

pub mod proton;

use crate::models::Game;

/// What kind of command we're launching into a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    /// The game's Proton directory could not be resolved (no prefix yet).
    NoProtonDir,
    /// The process failed to spawn.
    Spawn(std::io::ErrorKind),
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
            LaunchError::Spawn(kind) => write!(f, "failed to start process: {kind}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Launch a built-in Wine tool (e.g. `winecfg`) in the game's prefix.
pub fn launch_tool(game: &Game, tool: &str) -> Result<(), LaunchError> {
    proton::run_in_prefix(game, &[tool])
}
