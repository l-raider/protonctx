//! Steam discovery: locate the Steam installation, its library folders, installed apps,
//! and the compatibility tool each app runs with.
//!
//! Data sources (all read-only, parsed with `steam-vdf-parser`):
//! - `libraryfolders.vdf`  → library folder paths + which appids are installed where
//! - `appmanifest_<id>.acf` → installed app name + install dir
//! - `config.vdf`          → per-app compatibility tool mapping (`CompatToolMapping`)
//! - `compatdata/<id>/config_info` → authoritative Proton directory for a prefix

pub mod compat;
pub mod compatdata;
pub mod libraryfolders;
pub mod locations;
pub mod manifest;

use crate::models::Game;

/// Read-only error type for Steam discovery failures.
#[derive(Debug)]
pub enum SteamError {
    Io(std::io::Error),
    Parse(String),
    /// Steam could not be located (none of the known install paths exist).
    SteamNotFound,
}

impl std::fmt::Display for SteamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SteamError::Io(e) => write!(f, "I/O error: {e}"),
            SteamError::Parse(msg) => write!(f, "parse error: {msg}"),
            SteamError::SteamNotFound => write!(f, "Steam installation not found"),
        }
    }
}

impl std::error::Error for SteamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SteamError::Io(e) => Some(e),
            SteamError::Parse(_) | SteamError::SteamNotFound => None,
        }
    }
}

impl From<std::io::Error> for SteamError {
    fn from(e: std::io::Error) -> Self {
        SteamError::Io(e)
    }
}

/// Steam App ID for "Steamworks Common Redistributables", a shared non-game payload.
const STEAMWORKS_COMMON_REDISTRIBUTABLES_APPID: u32 = 228980;

/// Whether an install directory belongs to a Proton compatibility tool or a Steam
/// runtime (rather than a game). These ship a `toolmanifest.vdf` (Proton, runtimes)
/// or `compatibilitytool.vdf` (custom tools like GE-Proton) in their directory.
fn is_compat_tool(install_dir: &std::path::Path) -> bool {
    install_dir.join("toolmanifest.vdf").is_file()
        || install_dir.join("compatibilitytool.vdf").is_file()
}

/// The fallback library list used when `libraryfolders.vdf` is missing or yields no
/// valid folders: the Steam root itself (the default library), provided its
/// `steamapps/libraryfolders.vdf` exists. Returns an empty list otherwise.
///
/// The Steam root is returned *as* the library (not `<root>/steamapps`), because
/// `installed_apps()` appends `steamapps` to each library entry.
fn default_library_fallback(steam_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    if steam_root
        .join("steamapps")
        .join("libraryfolders.vdf")
        .is_file()
    {
        vec![steam_root.to_path_buf()]
    } else {
        Vec::new()
    }
}

/// Discover the installed Steam games across all library folders.
///
/// Returns `Err(SteamError::SteamNotFound)` when Steam cannot be located, so the
/// GUI can distinguish "Steam missing" from an empty library.
pub fn discover_games() -> Result<Vec<Game>, SteamError> {
    let steam_root = locations::find_steam_root().ok_or(SteamError::SteamNotFound)?;

    let libraries = match libraryfolders::library_folders(&steam_root) {
        Ok(libs) if !libs.is_empty() => libs,
        // Fall back to the default library. A "library" is the directory that
        // *contains* `steamapps/`, so the Steam root itself is the default
        // library (installed_apps() appends `steamapps` to each entry).
        _ => default_library_fallback(&steam_root),
    };

    let compat_tools = match compat::compat_tool_map(&steam_root) {
        Ok(map) => map,
        Err(e) => {
            // Non-fatal: a corrupt config.vdf just means we can't show per-game
            // tool names, not that discovery itself failed.
            eprintln!("protonctx: failed to parse config.vdf: {e}");
            std::collections::HashMap::new()
        }
    };

    let mut games = Vec::new();
    for library in &libraries {
        let apps = match manifest::installed_apps(library) {
            Ok(apps) => apps,
            Err(e) => {
                eprintln!("protonctx: failed to read manifests in {library:?}: {e}");
                continue;
            }
        };

        for app in apps {
            // Only include actual games. Steam installs several non-game "apps" in
            // the same manifests that we must filter out:
            //   - Compatibility tools (Proton, GE-Proton): marked by toolmanifest.vdf
            //     or compatibilitytool.vdf in their install directory.
            //   - Steam Linux Runtimes: marked by toolmanifest.vdf + VERSIONS.txt.
            //   - Steamworks Common Redistributables (appid 228980): shared payload.
            let Some(install_dir) = app.install_dir else {
                continue;
            };
            let common = library.join("steamapps").join("common").join(&install_dir);
            if !common.is_dir() {
                continue;
            }
            if is_compat_tool(&common) {
                continue;
            }
            if app.app_id == STEAMWORKS_COMMON_REDISTRIBUTABLES_APPID {
                continue;
            }

            let compat_tool = compat::compat_tool_for_app(&compat_tools, app.app_id);

            // Resolve the Proton directory for this game. The *selected* tool (from
            // config.vdf CompatToolMapping) is authoritative: it is what the row shows and
            // what a launch should use. The prefix's config_info records the tool that
            // *created* the prefix and goes stale when the user switches tools in Steam
            // without recreating the prefix, so it is only a fallback for games whose
            // selected tool cannot be located (e.g. a non-Proton layer like Boxtron).
            let proton_dir = crate::steam::compat::proton_dir_for_tool(&steam_root, &compat_tool)
                .or_else(|| {
                    compatdata::proton_dir_for(library, app.app_id).unwrap_or_else(|e| {
                        // A bad `compatdata/<id>/config_info` (e.g. permission denied)
                        // must not abort the whole discovery — just leave the prefix
                        // unresolved for this one game, like a manifest error does.
                        eprintln!(
                            "protonctx: failed to resolve proton dir for app {}: {e}",
                            app.app_id
                        );
                        None
                    })
                })
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            games.push(Game {
                name: app.name,
                app_id: app.app_id,
                compat_tool,
                library_path: library.to_string_lossy().into_owned(),
                proton_dir,
            });
        }
    }

    // Deterministic order for a stable UI.
    games.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.app_id.cmp(&b.app_id))
    });

    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fallback_uses_steam_root_as_library() {
        let root =
            std::env::temp_dir().join(format!("protonctx_test_fallback_{}", std::process::id()));
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        let libs = default_library_fallback(&root);
        assert_eq!(libs, vec![root.clone()]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn fallback_empty_when_no_steamapps_vdf() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_fallback_empty_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        assert!(default_library_fallback(&root).is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    // The returned library path must not itself contain a trailing `steamapps`,
    // otherwise installed_apps() would append another `steamapps` and look in
    // `<root>/steamapps/steamapps/...`.
    #[test]
    fn fallback_library_has_no_nested_steamapps() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_fallback_nested_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        let libs = default_library_fallback(&root);
        let lib: &PathBuf = &libs[0];
        assert!(!lib.ends_with("steamapps"));

        std::fs::remove_dir_all(&root).ok();
    }
}
