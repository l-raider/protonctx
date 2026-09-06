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
pub struct AppState {
    /// The absolute path of the last directory picked in the "Browse…" dialog, or
    /// `None` if none has been recorded yet.
    pub last_dir: Option<String>,
}

/// Return the home directory, if it can be determined.
///
/// Mirrors `home_dir` in `crate::steam::locations` so the whole crate resolves the
/// home directory the same way (honouring `$HOME`).
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolve an XDG base directory for the given environment variable, falling back to
/// the given `$HOME`-relative suffix when the variable is unset or empty.
///
/// `$XDG_CONFIG_HOME` and `$XDG_STATE_HOME` must be treated as *absolute* paths per the
/// spec; a relative value is ignored in favour of the default.
fn xdg_dir(env_var: &str, home_fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(env_var) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() && !dir.as_os_str().is_empty() {
            return Some(dir);
        }
    }
    home_dir().map(|home| home.join(home_fallback))
}

/// The directory that holds the config file, per XDG (default `~/.config`).
fn config_dir() -> Option<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

/// The directory that holds the state file, per XDG (default `~/.local/state`).
fn state_dir() -> Option<PathBuf> {
    xdg_dir("XDG_STATE_HOME", ".local/state")
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

/// Persist the user configuration. Returns `Ok(())` when written, and a best-effort
/// `Err` (carrying a human-readable reason) when the file could not be written.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err("could not resolve config directory".to_string());
    };
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_atomic(&path, &text).map_err(|e| format!("write config: {e}"))
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

/// Persist the remembered last directory. Best-effort; an `Err` carries a reason.
pub fn save_last_dir(dir: &Path) -> Result<(), String> {
    let Some(path) = state_path() else {
        return Err("could not resolve state directory".to_string());
    };
    let state = AppState {
        last_dir: Some(encode_dir(dir)),
    };
    let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    write_atomic(&path, &text).map_err(|e| format!("write state: {e}"))
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

/// Write `contents` to `path` atomically: write to a temporary sibling file and
/// `rename` it into place. This avoids leaving a truncated/corrupt file behind if
/// the process is killed (or the write fails) partway through, so a later load
/// never sees a half-written JSON document.
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
    std::fs::write(&tmp, contents)?;
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
}
