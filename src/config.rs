//! Application configuration and per-user state persistence.
//!
//! Two pieces of data are persisted as JSON, both under XDG Base Directory locations:
//! - `config.json` — user *preferences* (e.g. whether to remember the last directory),
//!   under `$XDG_CONFIG_HOME/protonctx/` (default `~/.config/protonctx/`).
//! - `state.json` — *transient state* (the last directory picked in the "Browse…"
//!   dialog), under `$XDG_STATE_HOME/protonctx/` (default `~/.local/state/protonctx/`).
//!
//! The XDG spec distinguishes config (persistent preferences) from state (data that
//! can be regenerated or lost without consequence), which is exactly the distinction
//! between the settings checkbox and the "last used directory" here.
//!
//! All operations are best-effort: a missing or unreadable file simply yields defaults
//! rather than failing. The GUI must never fail to start because a config file is
//! corrupt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::xdg;

/// The application's name, used for the `protonctx` subdirectory under each XDG root.
const APP_DIR: &str = "protonctx";

/// Persistent user preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Whether the "Browse…" file picker should remember (and reopen at) the last
    /// directory the user picked in. Defaults to `true`.
    pub remember_last_dir: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            remember_last_dir: true,
        }
    }
}

/// Transient per-user state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// The absolute path of the last directory picked in the "Browse…" dialog, or
    /// `None` if none has been recorded yet.
    pub last_dir: Option<String>,
}

/// The directory that holds the config file, per XDG (default `~/.config`).
fn config_dir() -> Option<PathBuf> {
    xdg::xdg_dir("XDG_CONFIG_HOME", ".config")
}

/// The directory that holds the state file, per XDG (default `~/.local/state`).
fn state_dir() -> Option<PathBuf> {
    xdg::xdg_dir("XDG_STATE_HOME", ".local/state")
}

/// The absolute path of the JSON config file, or `None` if it cannot be resolved.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(APP_DIR).join("config.json"))
}

/// The absolute path of the JSON state file, or `None` if it cannot be resolved.
pub fn state_path() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join(APP_DIR).join("state.json"))
}

/// Load the user configuration, falling back to defaults on any failure.
pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return AppConfig::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Persist the user configuration. Returns `Ok(())` when written; an `Err`
/// carries the I/O reason for the caller to prefix with its own context.
pub fn save_config(config: &AppConfig) -> std::io::Result<()> {
    let Some(path) = config_path() else {
        return Err(std::io::Error::other("could not resolve config directory"));
    };
    let text = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    write_atomic(&path, &text)
}

/// Load the remembered last directory, or `None` if unset/unreadable.
pub fn load_last_dir() -> Option<PathBuf> {
    let path = state_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let state: AppState = serde_json::from_str(&text).ok()?;
    state
        .last_dir
        .filter(|s| !s.is_empty())
        .map(|s| decode_dir(&s))
}

/// Persist the remembered last directory. Best-effort; an `Err` carries the I/O
/// reason for the caller to prefix with its own context.
pub fn save_last_dir(dir: &Path) -> std::io::Result<()> {
    let Some(path) = state_path() else {
        return Err(std::io::Error::other("could not resolve state directory"));
    };
    let state = AppState {
        last_dir: Some(encode_dir(dir)),
    };
    let text = serde_json::to_string_pretty(&state).map_err(std::io::Error::other)?;
    write_atomic(&path, &text)
}

/// Encode a directory path for storage in `state.json` without loss.
///
/// Paths that are valid UTF-8 are stored verbatim (so existing state files remain
/// readable); otherwise the raw OS bytes are hex-encoded with a `hex:` prefix so a
/// non-UTF-8 path (e.g. a directory containing invalid UTF-8 bytes) round-trips
/// exactly, rather than being mangled by `to_string_lossy()`.
fn encode_dir(dir: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let bytes = dir.as_os_str().as_bytes();
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => format!("hex:{}", hex_encode(bytes)),
    }
}

/// Reverse of [`encode_dir`]: a `hex:`-prefixed value is hex-decoded back to the
/// original bytes; anything else is treated as a plain UTF-8 path.
fn decode_dir(s: &str) -> PathBuf {
    if let Some(bytes) = s.strip_prefix("hex:").and_then(hex_decode) {
        use std::os::unix::ffi::OsStringExt;
        return PathBuf::from(std::ffi::OsString::from_vec(bytes));
    }
    PathBuf::from(s)
}

/// Hex-encode `bytes` to lowercase ASCII.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Hex-decode an even-length ASCII hex string, or `None` on malformed input.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Write `contents` to `path` atomically: write to a temporary sibling file, flush it
/// to disk, and `rename` it into place. This avoids leaving a truncated/corrupt file
/// behind if the process is killed (or the write fails) partway through, so a later load
/// never sees a half-written JSON document — and the `sync_all` ensures the *data* is
/// durable before the rename publishes it, so a crash cannot leave the target pointing
/// at an empty file.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Unique temp name: same directory (so rename stays on one filesystem) with a
    // process + thread id to avoid collisions between concurrent writers.
    let tmp = path.with_extension(format!(
        "tmp.{}.{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    // Write, then flush to disk *before* renaming: without the `sync_all` the
    // rename could be persisted ahead of the data on a crash, leaving an empty or
    // truncated file behind — exactly the corruption the atomic rename avoids.
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    // Atomically replace the target; `rename` is atomic on POSIX when src and dst
    // share a filesystem (guaranteed here since the temp file is a sibling).
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Best-effort cleanup of the temp file on failure.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_to_remember_last_dir_enabled() {
        let cfg = AppConfig::default();
        assert!(cfg.remember_last_dir);
    }

    #[test]
    fn state_defaults_to_no_last_dir() {
        let state = AppState::default();
        assert!(state.last_dir.is_none());
    }

    #[test]
    fn config_roundtrips_through_json() {
        let cfg = AppConfig {
            remember_last_dir: false,
        };
        let text = serde_json::to_string(&cfg).unwrap();
        assert_eq!(text, r#"{"remember_last_dir":false}"#);
        let back: AppConfig = serde_json::from_str(&text).unwrap();
        assert!(!back.remember_last_dir);
    }

    #[test]
    fn utf8_dir_is_stored_verbatim() {
        let dir = PathBuf::from("/home/user/.local/share/Steam");
        let encoded = encode_dir(&dir);
        assert_eq!(encoded, "/home/user/.local/share/Steam");
        assert_eq!(decode_dir(&encoded), dir);
    }

    #[test]
    fn non_utf8_dir_roundtrips_losslessly() {
        use std::os::unix::ffi::OsStringExt;
        // A path containing invalid UTF-8 bytes (0xFF) must round-trip exactly.
        let raw = b"/home/user/\xFFdir";
        let dir = PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()));

        let encoded = encode_dir(&dir);
        assert!(encoded.starts_with("hex:"));
        let decoded = decode_dir(&encoded);
        assert_eq!(decoded, dir);
    }

    #[test]
    fn hex_decode_rejects_odd_length() {
        assert!(hex_decode("abc").is_none());
        assert_eq!(hex_decode("0a0b").unwrap(), vec![0x0a, 0x0b]);
    }

    #[test]
    fn hex_decode_accepts_upper_case_and_rejects_non_hex_digits() {
        assert_eq!(hex_decode("0AfF").unwrap(), vec![0x0a, 0xff]);
        assert!(hex_decode("zz").is_none());
        assert!(hex_decode("0g").is_none());
        // A malformed `hex:` value is kept as a plain path.
        assert_eq!(decode_dir("hex:zz"), PathBuf::from("hex:zz"));
    }

    #[test]
    fn config_and_state_round_trip_under_the_xdg_dirs() {
        use crate::test_support::{is_child, run_in_child, temp_dir};
        if !is_child() {
            let dir = temp_dir("config_round_trip");
            let config = dir.join("config");
            let state = dir.join("state");
            run_in_child(
                "config::tests::config_and_state_round_trip_under_the_xdg_dirs",
                &[
                    ("XDG_CONFIG_HOME", config.as_os_str()),
                    ("XDG_STATE_HOME", state.as_os_str()),
                ],
                &[],
            );
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        let config = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap());
        let state = PathBuf::from(std::env::var_os("XDG_STATE_HOME").unwrap());
        let config_file = config.join("protonctx/config.json");
        let state_file = state.join("protonctx/state.json");
        assert_eq!(config_path(), Some(config_file.clone()));
        assert_eq!(state_path(), Some(state_file.clone()));

        // Nothing saved yet: defaults.
        assert!(load_config().remember_last_dir);
        assert_eq!(load_last_dir(), None);

        save_config(&AppConfig {
            remember_last_dir: false,
        })
        .unwrap();
        assert!(!load_config().remember_last_dir);
        // A corrupt file falls back to the defaults.
        std::fs::write(&config_file, "not json").unwrap();
        assert!(load_config().remember_last_dir);

        let picked = Path::new("/mnt/games/Trainer");
        save_last_dir(picked).unwrap();
        assert_eq!(load_last_dir().as_deref(), Some(picked));
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = PathBuf::from(std::ffi::OsString::from_vec(b"/mnt/\xFFdir".to_vec()));
            save_last_dir(&raw).unwrap();
            assert_eq!(load_last_dir(), Some(raw));
        }
        // An empty or corrupt entry reads as unset.
        std::fs::write(&state_file, r#"{"last_dir":""}"#).unwrap();
        assert_eq!(load_last_dir(), None);
        std::fs::write(&state_file, "not json").unwrap();
        assert_eq!(load_last_dir(), None);
        // No temp files are left behind by the atomic writes.
        assert_eq!(
            std::fs::read_dir(config.join("protonctx")).unwrap().count(),
            1
        );
        assert_eq!(
            std::fs::read_dir(state.join("protonctx")).unwrap().count(),
            1
        );
    }

    #[test]
    fn nothing_resolves_without_home_or_xdg_dirs() {
        use crate::test_support::{is_child, run_in_child};
        if !is_child() {
            return run_in_child(
                "config::tests::nothing_resolves_without_home_or_xdg_dirs",
                &[],
                &["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME"],
            );
        }
        assert_eq!(config_path(), None);
        assert_eq!(state_path(), None);
        assert!(load_config().remember_last_dir);
        assert_eq!(load_last_dir(), None);
        let error = save_config(&AppConfig::default()).unwrap_err();
        assert_eq!(error.to_string(), "could not resolve config directory");
        let error = save_last_dir(Path::new("/tmp")).unwrap_err();
        assert_eq!(error.to_string(), "could not resolve state directory");
    }

    #[test]
    fn write_atomic_reports_failures_and_cleans_up() {
        let dir = crate::test_support::temp_dir("config_write_atomic");
        std::fs::create_dir_all(&dir).unwrap();

        // The parent is a file, so it cannot be created as a directory.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "").unwrap();
        assert!(write_atomic(&blocker.join("config.json"), "{}").is_err());

        // The target is a non-empty directory: the rename fails and the temp
        // file is removed again.
        let target = dir.join("target.json");
        std::fs::create_dir_all(target.join("inner")).unwrap();
        assert!(write_atomic(&target, "{}").is_err());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        // A path with no parent: the temp file cannot be created at the root.
        assert!(write_atomic(Path::new("/"), "{}").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
