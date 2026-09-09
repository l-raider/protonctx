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

use std::sync::OnceLock;

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
pub fn resolve_host_path(path: &str) -> Option<String> {
    if !is_doc_path(path) {
        return None;
    }

    // The document portal lives on the host, so resolve there via
    // `flatpak-spawn --host`.
    let output = std::process::Command::new("flatpak-spawn")
        .arg("--host")
        .arg("flatpak")
        .arg("document-info")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    // `flatpak document-info` prints an `origin: <real path>` line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("origin: "))
        .map(|origin| origin.trim().to_string())
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
        assert!(is_doc_path("/run/user/1000/doc/O9IM4y7CjO949_dSnoxh2g/trainer.exe"));
        assert!(is_doc_path("/run/user/0/doc/abc123/file.txt"));
        // Not named `doc` after the uid, or not under /run/user at all.
        assert!(!is_doc_path("/run/user/1000/foo/trainer.exe"));
        assert!(!is_doc_path("/home/user/trainer.exe"));
        assert!(!is_doc_path("/run/other/1000/doc/x"));
    }

    #[test]
    fn resolve_host_path_ignores_non_doc_paths() {
        // Non-doc paths should pass through unchanged (return None) without
        // spawning any subprocess.
        assert_eq!(resolve_host_path("/home/user/trainer.exe"), None);
    }
}
