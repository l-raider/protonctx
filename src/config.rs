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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("write config: {e}"))
}

/// Load the remembered last directory, or `None` if unset/unreadable.
pub fn load_last_dir() -> Option<PathBuf> {
    let path = state_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let state: AppState = serde_json::from_str(&text).ok()?;
    state.last_dir.filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// Persist the remembered last directory. Best-effort; an `Err` carries a reason.
pub fn save_last_dir(dir: &Path) -> Result<(), String> {
    let Some(path) = state_path() else {
        return Err("could not resolve state directory".to_string());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create state dir: {e}"))?;
    }
    let state = AppState {
        last_dir: Some(dir.to_string_lossy().into_owned()),
    };
    let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("write state: {e}"))
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
}
