//! Invoke the `proton` launcher script to run an executable inside a prefix.
//!
//! This mirrors what the reference `proton-exec.sh` / `run-exe.sh` scripts do: set the
//! Steam compat environment variables and call `<proton_dir>/proton runinprefix <args>`.
//! The `runinprefix` verb runs the given command via the prefix's Wine, which is what we
//! want for arbitrary `.exe` files and Wine built-ins (winecfg, taskmgr, ...) alike.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::models::Game;

use super::{LaunchError, LaunchedProcess};

/// Run an arbitrary command (`.exe` path or Wine built-in) inside `game`'s Proton prefix.
///
/// `args` are the positional arguments passed after the `runinprefix` verb, and `cwd`
/// is the working directory to start in (`None` keeps protonctx's own). Both must
/// already be *host* paths: inside a Flatpak sandbox the command runs on the host, so
/// a document-portal alias has to be resolved first (see [`super::run_executable`]).
/// The caller checks that `cwd` exists, since inside the sandbox only the caller can
/// see the path the user picked. This spawns the process in the background
/// (non-blocking), matching how a GUI launcher should behave.
///
/// stdout/stderr are piped so the caller can stream their contents to the UI log; stdin is
/// left inherited so interactive executables that read from the terminal still work.
///
/// Returns the spawned [`LaunchedProcess`] so the caller can observe when the process
/// finishes and log its output. Note that `proton runinprefix` itself blocks until the
/// target executable exits (it invokes `subprocess.call`), so waiting on the returned child
/// tracks the lifetime of the launched executable, not just the wrapper script.
pub fn run_in_prefix(
    game: &Game,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<LaunchedProcess, LaunchError> {
    let proton = game.proton_script().ok_or(LaunchError::NoProtonDir)?;

    // Proton prefixes (compatdata) live under the library the game is installed in,
    // NOT the Steam root: a game on a secondary library keeps its prefix there.
    let compat_data =
        crate::steam::paths::app_compatdata_dir(Path::new(&game.library_path), game.app_id);
    let env = steam_env(game, &compat_data);

    // Inside a Flatpak sandbox the `proton` script lives on the *host* and must be
    // run there via `flatpak-spawn --host` (see `crate::flatpak`). On a normal host
    // install we invoke it directly. Both paths share the same Steam environment and
    // pipe the same stdout/stderr, so the caller-side watcher is unchanged.
    let flatpak = crate::flatpak::running_in_flatpak();
    let mut cmd = if flatpak {
        flatpak_spawn_command(&proton, args, env, cwd)
    } else {
        direct_command(&proton, args, env, cwd)
    };
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let command_line = format!(
        "{}{} runinprefix {} (prefix: {}{})",
        if flatpak { "flatpak-spawn --host " } else { "" },
        proton.display(),
        args.join(" "),
        compat_data.display(),
        cwd.map(|dir| format!(", in: {}", dir.display()))
            .unwrap_or_default(),
    );
    match cmd.spawn() {
        Ok(child) => Ok(LaunchedProcess {
            child,
            command_line,
        }),
        Err(source) => Err(LaunchError::Spawn {
            path: proton,
            source,
        }),
    }
}

/// The Steam compatibility environment for a launch in `game`'s prefix.
///
/// These four variables are what the reference `proton-exec.sh` / `run-exe.sh`
/// scripts set, and are required for `proton runinprefix` to resolve the correct
/// prefix and Wine. The client install is the Steam root, never the library: Proton
/// reads Steam client files from it while setting up the prefix.
fn steam_env(game: &Game, compat_data: &Path) -> [(&'static str, OsString); 4] {
    [
        ("STEAM_COMPAT_DATA_PATH", compat_data.as_os_str().to_owned()),
        (
            "STEAM_COMPAT_CLIENT_INSTALL_PATH",
            OsString::from(&game.steam_root),
        ),
        ("SteamGameId", game.app_id.to_string().into()),
        ("SteamAppId", game.app_id.to_string().into()),
    ]
}

/// Build the command that runs the Proton script directly on the host.
fn direct_command(
    proton: &Path,
    args: &[&str],
    env: [(&str, OsString); 4],
    cwd: Option<&Path>,
) -> Command {
    let mut cmd = Command::new(proton);
    cmd.arg("runinprefix");
    cmd.args(args);
    cmd.envs(env);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd
}

/// Build a `flatpak-spawn --host` command that runs the Proton script on the host.
///
/// The Steam environment is forwarded explicitly via `--env=K=V` rather than relying
/// on implicit propagation, so the host process always receives the exact prefix/root
/// values even across Flatpak's environment sanitization. stdout/stderr/exit-status are
/// forwarded by `flatpak-spawn` itself, matching the direct-launch semantics.
fn flatpak_spawn_command(
    proton: &Path,
    args: &[&str],
    env: [(&str, OsString); 4],
    cwd: Option<&Path>,
) -> Command {
    let mut cmd = Command::new("flatpak-spawn");
    cmd.arg("--host");

    // Forward the Steam environment to the host process explicitly.
    for (key, value) in env {
        // flatpak-spawn takes `--env=VAR=VALUE` as a single argument (not a
        // space-separated `--env VAR=VALUE` pair), so build the full token here.
        let mut arg = OsString::from("--env=");
        arg.push(key);
        arg.push("=");
        arg.push(value);
        cmd.arg(arg);
    }

    // The host process's working directory; like every flatpak-spawn option it
    // must come before the command.
    if let Some(dir) = cwd {
        let mut arg = OsString::from("--directory=");
        arg.push(dir);
        cmd.arg(arg);
    }

    // The command to run on the host: <proton> runinprefix <args...>.
    cmd.arg(proton);
    cmd.arg("runinprefix");
    cmd.args(args);

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game installed on a second library, with the Steam root elsewhere.
    fn game_on_second_library() -> Game {
        Game {
            steam_root: "/home/u/.local/share/Steam".to_string(),
            ..crate::test_support::game(
                "Broforce",
                274190,
                "proton_experimental",
                "/mnt/card",
                "/mnt/card/steamapps/common/Proton - Experimental",
            )
        }
    }

    fn env_of(game: &Game) -> [(&'static str, OsString); 4] {
        let compat_data =
            crate::steam::paths::app_compatdata_dir(Path::new(&game.library_path), game.app_id);
        steam_env(game, &compat_data)
    }

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn env_uses_the_steam_root_and_the_library_prefix() {
        let env = env_of(&game_on_second_library());
        let value = |key: &str| {
            env.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        assert_eq!(
            value("STEAM_COMPAT_CLIENT_INSTALL_PATH").as_deref(),
            Some("/home/u/.local/share/Steam")
        );
        assert_eq!(
            value("STEAM_COMPAT_DATA_PATH").as_deref(),
            Some("/mnt/card/steamapps/compatdata/274190")
        );
        assert_eq!(value("SteamAppId").as_deref(), Some("274190"));
        assert_eq!(value("SteamGameId").as_deref(), Some("274190"));
    }

    #[test]
    fn direct_command_sets_the_env_and_working_directory() {
        let game = game_on_second_library();
        let proton = Path::new("/mnt/card/steamapps/common/Proton - Experimental/proton");
        let dir = Path::new("/games/Trainer");

        let cmd = direct_command(proton, &["/games/Trainer/t.exe"], env_of(&game), Some(dir));
        assert_eq!(cmd.get_program(), proton.as_os_str());
        assert_eq!(args_of(&cmd), ["runinprefix", "/games/Trainer/t.exe"]);
        assert_eq!(cmd.get_current_dir(), Some(dir));
        let root = cmd
            .get_envs()
            .find(|(key, _)| *key == "STEAM_COMPAT_CLIENT_INSTALL_PATH")
            .and_then(|(_, value)| value);
        assert_eq!(
            root,
            Some(std::ffi::OsStr::new("/home/u/.local/share/Steam"))
        );

        let cmd = direct_command(proton, &["winecfg"], env_of(&game), None);
        assert_eq!(cmd.get_current_dir(), None);
    }

    #[test]
    fn flatpak_command_puts_options_before_the_proton_script() {
        let game = game_on_second_library();
        let proton = Path::new("/mnt/card/steamapps/common/Proton - Experimental/proton");

        let cmd = flatpak_spawn_command(
            proton,
            &["/games/Trainer/t.exe"],
            env_of(&game),
            Some(Path::new("/games/Trainer")),
        );
        assert_eq!(cmd.get_program(), "flatpak-spawn");
        assert_eq!(
            args_of(&cmd),
            [
                "--host",
                "--env=STEAM_COMPAT_DATA_PATH=/mnt/card/steamapps/compatdata/274190",
                "--env=STEAM_COMPAT_CLIENT_INSTALL_PATH=/home/u/.local/share/Steam",
                "--env=SteamGameId=274190",
                "--env=SteamAppId=274190",
                "--directory=/games/Trainer",
                "/mnt/card/steamapps/common/Proton - Experimental/proton",
                "runinprefix",
                "/games/Trainer/t.exe",
            ]
        );

        let cmd = flatpak_spawn_command(proton, &["winecfg"], env_of(&game), None);
        assert!(
            !args_of(&cmd)
                .iter()
                .any(|arg| arg.starts_with("--directory"))
        );
    }
}
