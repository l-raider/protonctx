//! Detect whether protonctx is running inside a Flatpak sandbox.
//!
//! The launcher must behave differently inside a Flatpak sandbox: the `proton`
//! script lives on the *host* (under `~/.local/share/Steam/...`) and cannot be
//! executed directly from within the sandbox — it must be run on the host via
//! `flatpak-spawn --host`. Detection therefore gates how [`crate::launcher`]
//! spawns the Proton process.
//!
//! # Detection signals
//!
//! Two signals are reliable and exist *only* inside a Flatpak sandbox:
//!
//! - [`/.flatpak-info`](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
//!   is the effective metadata file Flatpak always exposes inside a running app
//!   (also symlinked at `/run/user/$UID/flatpak-info` for older releases).
//! - The `FLATPAK_ID` environment variable is set by `flatpak run` to the
//!   application ID of the running app.
//!
//! Both are checked so a sandbox is still detected if one signal is absent on an
//! unusual installation. Outside a sandbox neither is present.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long a host query (the document portal) may take before it is given up.
const HOST_QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether the process is running inside a Flatpak sandbox.
///
/// The result is computed once and cached: the check is cheap, but the answer can
/// never change during the lifetime of a process, so caching avoids repeating the
/// filesystem probe on every launch.
pub fn running_in_flatpak() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(detect)
}

/// Perform the actual detection (see module docs for the signals used).
fn detect() -> bool {
    // The canonical marker: Flatpak mounts its effective metadata here in every
    // running sandbox.
    if std::path::Path::new("/.flatpak-info").exists() {
        return true;
    }
    // Fallback signal: `flatpak run` sets this to the app ID.
    std::env::var_os("FLATPAK_ID").is_some()
}

/// The application ID advertised to the display server for protonctx windows.
///
/// Wayland compositors match a window to its `.desktop` file by `app_id` and use
/// the entry's `Icon=` for the titlebar and window menu. Advertising an identity
/// with no matching desktop file makes KWin fall back to its generic "wayland"
/// icon (see `XdgToplevelWindow::updateIcon()`), even when the icon is
/// installed under a different name. The desktop entry that exists depends on
/// how protonctx was installed:
///
/// - Inside a Flatpak sandbox `FLATPAK_ID` is authoritative; the exported entry
///   is `io.github.l_raider.protonctx.desktop`.
/// - Native packaging (deb/rpm/AppImage) installs `protonctx.desktop`.
/// - A locally built binary on a host where only the Flatpak is installed finds
///   just the reverse-DNS entry, so following it is what makes the titlebar
///   icon resolve for `cargo build` runs too.
///
/// The first installed entry wins. If none is installed the native ID is used
/// and the compositor shows its fallback icon, exactly as for any other
/// application that has not been installed.
pub fn window_app_id() -> String {
    let flatpak_id = std::env::var("FLATPAK_ID").ok();
    app_id_for(
        running_in_flatpak(),
        flatpak_id.as_deref(),
        desktop_entry_installed,
    )
}

/// Pure decision function behind [`window_app_id`], kept separate so the
/// Flatpak/native/installed-entry mapping can be tested without touching the
/// environment or the filesystem.
fn app_id_for(
    flatpak: bool,
    flatpak_id: Option<&str>,
    entry_installed: impl Fn(&str) -> bool,
) -> String {
    /// App ID used by the deb/rpm/AppImage packaging, matching
    /// `packaging/protonctx.desktop`.
    const NATIVE_APP_ID: &str = "protonctx";
    /// App ID used by the Flatpak packaging and its exported desktop entry.
    const FLATPAK_APP_ID: &str = "io.github.l_raider.protonctx";

    if flatpak && let Some(id) = flatpak_id.filter(|id| !id.is_empty()) {
        return id.to_string();
    }

    [NATIVE_APP_ID, FLATPAK_APP_ID]
        .into_iter()
        .find(|id| entry_installed(id))
        .unwrap_or(NATIVE_APP_ID)
        .to_string()
}

/// Whether `<data dir>/applications/<id>.desktop` exists in any XDG data
/// directory. `XDG_DATA_HOME` and `XDG_DATA_DIRS` are honoured, including their
/// spec defaults, and Flatpak's export directory is normally listed in
/// `XDG_DATA_DIRS` on the host.
fn desktop_entry_installed(id: &str) -> bool {
    let file = format!("{id}.desktop");

    let user_data = crate::xdg::xdg_dir("XDG_DATA_HOME", ".local/share");
    if user_data.is_some_and(|dir| dir.join("applications").join(&file).is_file()) {
        return true;
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    data_dirs.split(':').any(|dir| {
        let dir = PathBuf::from(dir);
        dir.is_absolute() && dir.join("applications").join(&file).is_file()
    })
}

/// Resolve a path returned by the file-chooser portal to its real host origin, so
/// it can be passed to a host-side process via `flatpak-spawn --host`.
///
/// Inside the sandbox, picking a file the sandbox has no direct filesystem access
/// to (e.g. an `.exe` on a Steam library outside `$HOME`) yields a *document
/// portal* path of the form `/run/user/$UID/doc/$DOC_ID/<name>` — a read-only
/// FUSE alias that is only meaningful inside the sandbox. Wine running on the host
/// cannot open that alias ("file not found"). This resolves it back to the real
/// path by asking the host's document portal (`flatpak document-info`, itself
/// spawned on the host via `flatpak-spawn --host`) for the document's `origin:`.
///
/// Returns `None` for paths that need no translation (anything that is not a
/// document-portal alias), and when resolution fails — in which case the caller
/// falls back to using the path verbatim. This makes the call safe to apply
/// unconditionally to every launch argument when inside a sandbox.
///
/// # Threading
///
/// This performs a **blocking** subprocess call (`flatpak-spawn --host flatpak
/// document-info`), so call it off the UI thread: [`crate::launcher::resolve_executable`]
/// runs on a worker. Non-document paths return early without spawning anything. The
/// call gives up after [`HOST_QUERY_TIMEOUT`], so a hung host D-Bus or portal costs
/// the fallback path, not a stuck launch; `--watch-bus` asks the host to end the
/// query when the killed `flatpak-spawn` goes away.
pub fn resolve_host_path(path: &str) -> Option<String> {
    if !is_doc_path(path) {
        return None;
    }

    // The document portal lives on the host, so resolve there via
    // `flatpak-spawn --host`.
    let mut cmd = Command::new("flatpak-spawn");
    cmd.args(["--host", "--watch-bus", "flatpak", "document-info"])
        .arg(path);
    let output = output_within(cmd, HOST_QUERY_TIMEOUT)?;
    if !output.status.success() {
        return None;
    }
    document_origin(&String::from_utf8_lossy(&output.stdout))
}

/// The real path in `flatpak document-info` output: its `origin: <path>` line.
fn document_origin(info: &str) -> Option<String> {
    info.lines()
        .find_map(|line| line.strip_prefix("origin: "))
        .map(|origin| origin.trim().to_string())
}

/// Run `cmd` and collect its stdout, or kill it and return `None` once
/// `timeout` passes (std's `Command` has no timeout of its own). Meant for
/// commands with short output: stdout is read only after the exit, so output
/// beyond a pipe buffer would stall the command until the timeout.
fn output_within(mut cmd: Command, timeout: Duration) -> Option<Output> {
    /// How often the child is checked for an exit.
    const POLL: Duration = Duration::from_millis(10);

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Whether `path` is a document-portal alias (`/run/user/<uid>/doc/<docid>/...`).
fn is_doc_path(path: &str) -> bool {
    // /run/user/<uid>/doc/<...>
    path.strip_prefix("/run/user/")
        .map(|rest| rest.split('/').nth(1) == Some("doc"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_is_stable_across_calls() {
        // The cached value must be identical on every call (the answer cannot
        // change mid-process).
        let first = running_in_flatpak();
        let second = running_in_flatpak();
        assert_eq!(first, second);
    }

    #[test]
    fn detection_returns_a_plain_bool() {
        // Just documents the contract: a bool, not a Result — callers never need
        // to handle a detection error.
        let _: bool = running_in_flatpak();
    }

    #[test]
    fn doc_path_detection() {
        assert!(is_doc_path(
            "/run/user/1000/doc/O9IM4y7CjO949_dSnoxh2g/trainer.exe"
        ));
        assert!(is_doc_path("/run/user/0/doc/abc123/file.txt"));
        // Not named `doc` after the uid, or not under /run/user at all.
        assert!(!is_doc_path("/run/user/1000/foo/trainer.exe"));
        assert!(!is_doc_path("/home/user/trainer.exe"));
        assert!(!is_doc_path("/run/other/1000/doc/x"));
    }

    #[test]
    fn document_origin_reads_the_origin_line() {
        let info = "id: O9IM4y7CjO949_dSnoxh2g\n\
                    path: /run/user/1000/doc/O9IM4y7CjO949_dSnoxh2g/trainer.exe\n\
                    origin: /mnt/games/Trainer/trainer.exe\n\
                    permissions:\n";
        assert_eq!(
            document_origin(info).as_deref(),
            Some("/mnt/games/Trainer/trainer.exe")
        );
        assert_eq!(document_origin("id: x\n"), None);
    }

    #[test]
    fn output_within_returns_output_of_a_quick_command() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo 'origin: /x'"]);
        let output = output_within(cmd, Duration::from_secs(10)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"origin: /x\n");
    }

    #[test]
    fn output_within_gives_up_on_a_hung_command() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        assert!(output_within(cmd, Duration::from_millis(50)).is_none());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn resolve_host_path_ignores_non_doc_paths() {
        // Non-doc paths should pass through unchanged (return None) without
        // spawning any subprocess.
        assert_eq!(resolve_host_path("/home/user/trainer.exe"), None);
    }

    #[test]
    fn app_id_uses_flatpak_id_inside_sandbox() {
        // Inside a sandbox the desktop file is exported under the Flatpak app
        // ID, so the window must advertise exactly that ID for the compositor
        // to find the icon.
        assert_eq!(
            app_id_for(true, Some("io.github.l_raider.protonctx"), |_| false),
            "io.github.l_raider.protonctx"
        );
    }

    #[test]
    fn app_id_prefers_the_native_entry() {
        // With both entries installed (native package + Flatpak) the native
        // binary keeps grouping with its own launcher.
        assert_eq!(app_id_for(false, None, |_| true), "protonctx");
    }

    #[test]
    fn app_id_follows_an_installed_flatpak_entry() {
        // A locally built binary on a Flatpak host only finds the exported
        // reverse-DNS entry; advertising it is what makes KWin resolve `Icon=`.
        assert_eq!(
            app_id_for(false, None, |id| id == "io.github.l_raider.protonctx"),
            "io.github.l_raider.protonctx"
        );
    }

    #[test]
    fn app_id_falls_back_to_native_id() {
        // No entry installed (or missing/empty FLATPAK_ID): use the native ID;
        // the compositor shows its fallback icon, as for any uninstalled app.
        assert_eq!(app_id_for(false, None, |_| false), "protonctx");
        assert_eq!(app_id_for(true, None, |_| false), "protonctx");
        assert_eq!(app_id_for(true, Some(""), |_| false), "protonctx");
    }
}
