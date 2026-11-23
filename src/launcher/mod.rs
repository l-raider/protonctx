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

/// An executable the user picked, resolved for a launch: the path the host sees,
/// and the folder to start in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    pub host_path: PathBuf,
    pub cwd: Option<PathBuf>,
}

/// Resolve a picked executable for [`run_executable`].
///
/// Windows launchers (Explorer, Steam) start a program in its own folder, and many
/// trainers, mod managers and installers open their files by relative path, so the
/// launch does the same. When the folder cannot be seen the launch still goes ahead,
/// from protonctx's own working directory.
///
/// Inside a Flatpak sandbox `picked` may be a document-portal alias; it is resolved to
/// its host path (see [`crate::flatpak::resolve_host_path`]), since the command runs
/// on the host. That asks the host and can take seconds, so this **blocks**: call it
/// off the UI thread.
pub fn resolve_executable(picked: &Path) -> Executable {
    let host_path = if crate::flatpak::running_in_flatpak() {
        crate::flatpak::resolve_host_path(&picked.to_string_lossy())
            .map_or_else(|| picked.to_path_buf(), PathBuf::from)
    } else {
        picked.to_path_buf()
    };
    let cwd = working_dir(picked, &host_path).map(Path::to_path_buf);
    Executable { host_path, cwd }
}

/// Run a resolved executable in `game`'s prefix.
pub fn run_executable(game: &Game, exe: &Executable) -> Result<LaunchedProcess, LaunchError> {
    proton::run_in_prefix(
        game,
        &[&exe.host_path.to_string_lossy()],
        exe.cwd.as_deref(),
    )
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
    use crate::test_support::{game, is_child, run_in_child, temp_dir};

    /// Write an executable shell script with `body` to `path`.
    pub(super) fn write_script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A game under `root` whose `proton` is a stand-in script that echoes its
    /// arguments, prefix, and working directory.
    pub(super) fn game_with_fake_proton(root: &Path) -> Game {
        let proton_dir = root.join("Proton");
        write_script(
            &proton_dir.join("proton"),
            "echo \"$@\"\necho \"prefix=$STEAM_COMPAT_DATA_PATH\"\necho \"cwd=$(pwd)\"",
        );
        game(
            "Fake",
            42,
            "proton_9",
            root.join("lib").to_str().unwrap(),
            proton_dir.to_str().unwrap(),
        )
    }

    /// Wait for a launch and return its stdout.
    pub(super) fn output_of(launched: LaunchedProcess) -> String {
        let output = launched.child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn launch_errors_describe_themselves_and_their_source() {
        use std::error::Error as _;
        let missing = LaunchError::NoProtonDir;
        assert_eq!(
            missing.to_string(),
            "no Proton directory found for this game (has it been run yet?)"
        );
        assert!(missing.source().is_none());

        let spawn = LaunchError::Spawn {
            path: PathBuf::from("/x/proton"),
            source: std::io::Error::other("denied"),
        };
        assert_eq!(
            spawn.to_string(),
            "failed to start process `/x/proton`: denied"
        );
        assert_eq!(spawn.source().unwrap().to_string(), "denied");
    }

    #[test]
    fn a_tool_launch_needs_a_proton_dir() {
        let game = game("No Prefix", 1, "", "/lib", "");
        assert!(matches!(
            launch_tool(&game, "winecfg"),
            Err(LaunchError::NoProtonDir)
        ));
    }

    #[test]
    fn tools_and_executables_run_in_the_prefix_on_the_host() {
        // Outside a sandbox; the setup happens in the child, which owns its
        // temp dir.
        if !is_child() {
            return run_in_child(
                "launcher::tests::tools_and_executables_run_in_the_prefix_on_the_host",
                &[],
                &["FLATPAK_ID"],
            );
        }
        let root = temp_dir("launcher_direct_launch");
        let game = game_with_fake_proton(&root);

        let launched = launch_tool(&game, "winecfg").unwrap();
        assert!(launched.command_line.ends_with(&format!(
            "proton runinprefix winecfg (prefix: {})",
            root.join("lib/steamapps/compatdata/42").display()
        )));
        let output = output_of(launched);
        assert!(output.starts_with("runinprefix winecfg\n"), "{output}");

        let folder = root.join("Trainer");
        std::fs::create_dir_all(&folder).unwrap();
        let exe = resolve_executable(&folder.join("t.exe"));
        let launched = run_executable(&game, &exe).unwrap();
        assert!(
            launched
                .command_line
                .ends_with(&format!(", in: {})", folder.display()))
        );
        let output = output_of(launched);
        assert!(
            output.contains(&format!("cwd={}\n", folder.display())),
            "{output}"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn working_dir_is_the_host_folder_when_the_picked_folder_exists() {
        let dir = temp_dir("working_dir");
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
    fn a_host_pick_resolves_to_itself_and_its_folder() {
        // A path outside the document portal needs no resolving, even in a
        // sandbox.
        let dir = temp_dir("resolve_executable");
        std::fs::create_dir_all(&dir).unwrap();
        let picked = dir.join("trainer.exe");

        assert_eq!(
            resolve_executable(&picked),
            Executable {
                host_path: picked.clone(),
                cwd: Some(dir.clone()),
            }
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_portal_pick_in_a_sandbox_resolves_to_its_host_path() {
        if !is_child() {
            let bin = temp_dir("launcher_portal_bin");
            write_script(
                &bin.join("flatpak-spawn"),
                "case \"$5\" in *fail*) exit 1;; esac\necho \"origin: /mnt/games/Trainer/t.exe\"",
            );
            run_in_child(
                "launcher::tests::a_portal_pick_in_a_sandbox_resolves_to_its_host_path",
                &[
                    ("FLATPAK_ID", "io.example.Test".as_ref()),
                    ("PATH", bin.as_os_str()),
                ],
                &[],
            );
            std::fs::remove_dir_all(&bin).ok();
            return;
        }
        // The alias folder is not visible here, so no working directory.
        assert_eq!(
            resolve_executable(Path::new("/run/user/1000/doc/abc/t.exe")),
            Executable {
                host_path: PathBuf::from("/mnt/games/Trainer/t.exe"),
                cwd: None,
            }
        );
        // An unresolvable alias is used verbatim.
        let alias = Path::new("/run/user/1000/doc/fail/t.exe");
        assert_eq!(resolve_executable(alias).host_path, alias);
    }

    #[test]
    fn working_dir_is_unset_when_the_folder_is_missing() {
        let missing = temp_dir("working_dir_missing").join("t.exe");
        assert_eq!(working_dir(&missing, &missing), None);
        // A bare file name has no folder to start in.
        assert_eq!(working_dir(Path::new("t.exe"), Path::new("t.exe")), None);
    }
}
