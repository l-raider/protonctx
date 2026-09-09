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
}
