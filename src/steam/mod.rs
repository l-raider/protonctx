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
pub mod paths;
pub mod shadercache;

use std::collections::HashMap;

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
    if paths::steamapps_dir(steam_root)
        .join(paths::LIBRARYFOLDERS_VDF)
        .is_file()
    {
        vec![steam_root.to_path_buf()]
    } else {
        Vec::new()
    }
}

/// The libraries to scan: those `libraryfolders.vdf` lists, or the default
/// library when it lists none. An unreadable or corrupt file is reported in
/// `warnings`, since games on the other libraries then go missing.
fn libraries_of(
    steam_root: &std::path::Path,
    warnings: &mut Vec<String>,
) -> Vec<std::path::PathBuf> {
    match libraryfolders::library_folders(steam_root) {
        Ok(libs) if !libs.is_empty() => libs,
        result => {
            if let Err(e) = result {
                warnings.push(format!(
                    "Failed to read the Steam library list, scanning only the default library: {e}"
                ));
            }
            // A "library" is the directory that *contains* `steamapps/`, so
            // the Steam root itself is the default library (installed_apps()
            // appends `steamapps` to each entry).
            default_library_fallback(steam_root)
        }
    }
}

/// The outcome of a successful discovery scan: the games plus any non-fatal
/// problems worth surfacing to the user (corrupt `config.vdf`, unreadable
/// manifest directories, unresolvable prefixes).
///
/// Warnings are returned rather than printed so the GUI can show them in the log
/// panel; `eprintln!` alone is invisible to a desktop-launched app.
#[derive(Debug, Default)]
pub struct Discovery {
    pub games: Vec<Game>,
    pub warnings: Vec<String>,
}

/// Maximum number of per-app warnings collected in one scan, so a library with a
/// systematically broken prefix directory cannot produce unbounded output.
const MAX_WARNINGS: usize = 20;

/// Discover the installed Steam games across all library folders.
///
/// Returns `Err(SteamError::SteamNotFound)` when Steam cannot be located, so the
/// GUI can distinguish "Steam missing" from an empty library. Non-fatal problems
/// are reported in [`Discovery::warnings`] instead of aborting the scan.
pub fn discover_games() -> Result<Discovery, SteamError> {
    let steam_root = locations::find_steam_root().ok_or(SteamError::SteamNotFound)?;
    Ok(discover_games_in(steam_root))
}

/// [`discover_games`] for an already-located Steam root.
fn discover_games_in(steam_root: std::path::PathBuf) -> Discovery {
    let mut warnings: Vec<String> = Vec::new();

    let libraries = libraries_of(&steam_root, &mut warnings);

    let compat_tools = match compat::compat_tool_map(&steam_root) {
        Ok(map) => map,
        Err(e) => {
            // Non-fatal: a corrupt config.vdf just means we can't show per-game
            // tool names, not that discovery itself failed.
            warnings.push(format!("Failed to parse config.vdf: {e}"));
            HashMap::new()
        }
    };

    let mut games = Vec::new();
    // Built once for the whole scan: the internal-name → install-dir map for
    // built-in Proton tools. Resolving it per game would re-read and VDF-parse
    // every appmanifest on each iteration (O(games × manifests)). Proton installs
    // into the default library, which is not always the root, so every library
    // is searched; the root too, in case `libraryfolders.vdf` leaves it out.
    let mut tool_libraries = libraries.clone();
    if !tool_libraries.contains(&steam_root) {
        tool_libraries.push(steam_root.clone());
    }
    let builtin_tools = compat::builtin_tool_dirs(&tool_libraries);
    let steam_root_text = steam_root.to_string_lossy().into_owned();
    for library in &libraries {
        let apps = match manifest::installed_apps(library) {
            Ok(apps) => apps,
            Err(e) => {
                warnings.push(format!("Failed to read manifests in {library:?}: {e}"));
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
            let common = paths::app_common_dir(library, &install_dir);
            if !common.is_dir() {
                continue;
            }
            if is_compat_tool(&common) {
                continue;
            }
            if app.app_id == STEAMWORKS_COMMON_REDISTRIBUTABLES_APPID {
                continue;
            }

            // Steam's global default tool only covers games without a native
            // build; an existing prefix is the sign a game runs under Proton.
            let has_prefix = paths::app_compatdata_dir(library, app.app_id).is_dir();
            let compat_tool = compat::compat_tool_for_app(&compat_tools, app.app_id, has_prefix);

            // Resolve the Proton directory for this game. The *selected* tool (from
            // config.vdf CompatToolMapping) is authoritative: it is what the row shows and
            // what a launch should use. The prefix's config_info records the tool that
            // *created* the prefix and goes stale when the user switches tools in Steam
            // without recreating the prefix, so it is only a fallback for games whose
            // selected tool cannot be located (e.g. a non-Proton layer like Boxtron).
            let proton_dir = compat::proton_dir_for_tool(&steam_root, &builtin_tools, &compat_tool)
                .or_else(|| {
                    compatdata::proton_dir_for(library, app.app_id).unwrap_or_else(|e| {
                        // A bad `compatdata/<id>/config_info` (e.g. permission denied)
                        // must not abort the whole discovery — just leave the prefix
                        // unresolved for this one game, like a manifest error does.
                        if warnings.len() < MAX_WARNINGS {
                            warnings.push(format!(
                                "Failed to resolve proton dir for app {}: {e}",
                                app.app_id
                            ));
                        }
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
                steam_root: steam_root_text.clone(),
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

    Discovery { games, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{is_child, run_in_child, temp_dir};
    use std::path::{Path, PathBuf};

    #[test]
    fn fallback_uses_steam_root_as_library() {
        let root = crate::test_support::temp_dir("fallback");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        let libs = default_library_fallback(&root);
        assert_eq!(libs, vec![root.clone()]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn fallback_empty_when_no_steamapps_vdf() {
        let root = crate::test_support::temp_dir("fallback_empty");
        std::fs::create_dir_all(&root).unwrap();

        assert!(default_library_fallback(&root).is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    // The returned library path must not itself contain a trailing `steamapps`,
    // otherwise installed_apps() would append another `steamapps` and look in
    // `<root>/steamapps/steamapps/...`.
    #[test]
    fn fallback_library_has_no_nested_steamapps() {
        let root = crate::test_support::temp_dir("fallback_nested");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        let libs = default_library_fallback(&root);
        let lib: &PathBuf = &libs[0];
        assert!(!lib.ends_with("steamapps"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_corrupt_library_list_is_reported_and_the_root_is_scanned() {
        let root = crate::test_support::temp_dir("libraries_corrupt");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(
            root.join("config").join("libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n",
        )
        .unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        let mut warnings = Vec::new();
        assert_eq!(libraries_of(&root, &mut warnings), vec![root.clone()]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("library list"), "{warnings:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_readable_library_list_adds_no_warning() {
        let root = crate::test_support::temp_dir("libraries_ok");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("config").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                root.display()
            ),
        )
        .unwrap();

        let mut warnings = Vec::new();
        assert_eq!(libraries_of(&root, &mut warnings), vec![root.clone()]);
        assert!(warnings.is_empty(), "{warnings:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_defaults_to_no_games_and_no_warnings() {
        // Documents the contract the GUI relies on: a `Discovery` is always safe
        // to iterate (empty by default) and warnings are separate from games.
        let discovery = Discovery::default();
        assert!(discovery.games.is_empty());
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn errors_describe_themselves() {
        let io = SteamError::from(std::io::Error::other("disk on fire"));
        assert!(matches!(io, SteamError::Io(_)));
        assert_eq!(io.to_string(), "I/O error: disk on fire");
        assert_eq!(
            SteamError::Parse("bad vdf".to_string()).to_string(),
            "parse error: bad vdf"
        );
        assert_eq!(
            SteamError::SteamNotFound.to_string(),
            "Steam installation not found"
        );
    }

    #[test]
    fn only_io_errors_have_a_source() {
        use std::error::Error as _;
        let io = SteamError::Io(std::io::Error::other("disk on fire"));
        assert_eq!(io.source().unwrap().to_string(), "disk on fire");
        assert!(SteamError::Parse(String::new()).source().is_none());
        assert!(SteamError::SteamNotFound.source().is_none());
    }

    #[test]
    fn an_empty_library_list_falls_back_without_a_warning() {
        let root = temp_dir("libraries_empty");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        // Valid, but its only folder does not exist.
        std::fs::write(
            root.join("steamapps").join("libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\"/nonexistent/protonctx\"\n\t}\n}\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        assert_eq!(libraries_of(&root, &mut warnings), vec![root.clone()]);
        assert!(warnings.is_empty(), "{warnings:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Write `<library>/steamapps/appmanifest_<app_id>.acf`, with an
    /// `installdir` only when `install_dir` is given.
    fn write_manifest(library: &Path, app_id: u32, name: &str, install_dir: Option<&str>) {
        let steamapps = library.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        let install_dir = install_dir
            .map(|dir| format!("\t\"installdir\"\t\"{dir}\"\n"))
            .unwrap_or_default();
        std::fs::write(
            steamapps.join(format!("appmanifest_{app_id}.acf")),
            format!("\"AppState\"\n{{\n\t\"appid\"\t\"{app_id}\"\n\t\"name\"\t\"{name}\"\n{install_dir}}}\n"),
        )
        .unwrap();
    }

    /// Write a game's manifest and create its `steamapps/common/<install_dir>`.
    fn install_game(library: &Path, app_id: u32, name: &str) -> PathBuf {
        let install_dir = format!("game{app_id}");
        write_manifest(library, app_id, name, Some(&install_dir));
        let common = paths::app_common_dir(library, &install_dir);
        std::fs::create_dir_all(&common).unwrap();
        common
    }

    /// Write `compatdata/<app_id>/config_info` holding raw `content`.
    fn write_config_info(library: &Path, app_id: u32, content: &[u8]) {
        let prefix = paths::app_compatdata_dir(library, app_id);
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::write(prefix.join("config_info"), content).unwrap();
    }

    #[test]
    fn discovery_lists_games_and_skips_non_games() {
        let base = temp_dir("discover_full");
        let root = base.join("Steam");
        let second = base.join("library");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        // The root is not listed, so it is added for tool lookup only.
        std::fs::write(
            root.join("config").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
                second.display()
            ),
        )
        .unwrap();
        std::fs::write(
            root.join("config").join("config.vdf"),
            "\"InstallConfigStore\"\n{\n\"Software\"\n{\n\"Valve\"\n{\n\"Steam\"\n{\n\
             \"CompatToolMapping\"\n{\n\"10\"\n{\n\"name\"\t\"GE-Proton10-34\"\n}\n\
             \"30\"\n{\n\"name\"\t\"boxtron\"\n}\n}\n}\n}\n}\n}\n",
        )
        .unwrap();
        let ge = root.join("compatibilitytools.d").join("GE-Proton10-34");
        std::fs::create_dir_all(&ge).unwrap();

        // Games: a mapped custom tool, a native game, a prefix-only fallback,
        // and two names differing only in case (ordered by app id).
        install_game(&second, 10, "beta");
        install_game(&second, 20, "Alpha");
        install_game(&second, 30, "gamma");
        install_game(&second, 41, "Delta");
        install_game(&second, 40, "delta");
        let recorded = second.join("steamapps").join("common").join("Proton 9.0");
        std::fs::create_dir_all(&recorded).unwrap();
        write_config_info(
            &second,
            30,
            format!("{}/files/share/default_pfx/\n", recorded.display()).as_bytes(),
        );

        // Non-games: no installdir, an uninstalled dir, Proton, a custom tool,
        // and the Steamworks redistributables.
        write_manifest(&second, 50, "No Dir", None);
        write_manifest(&second, 51, "Not Installed", Some("missing"));
        let proton = install_game(&second, 52, "Proton Experimental");
        std::fs::write(proton.join("toolmanifest.vdf"), "x").unwrap();
        let custom = install_game(&second, 53, "Custom Tool");
        std::fs::write(custom.join("compatibilitytool.vdf"), "x").unwrap();
        install_game(&second, STEAMWORKS_COMMON_REDISTRIBUTABLES_APPID, "Redist");

        let discovery = discover_games_in(root.clone());
        assert!(discovery.warnings.is_empty(), "{:?}", discovery.warnings);
        let summary: Vec<_> = discovery
            .games
            .iter()
            .map(|g| (g.name.as_str(), g.app_id, g.compat_tool.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Alpha", 20, ""),
                ("beta", 10, "GE-Proton10-34"),
                ("delta", 40, ""),
                ("Delta", 41, ""),
                ("gamma", 30, "boxtron"),
            ]
        );
        let game = |id| discovery.games.iter().find(|g| g.app_id == id).unwrap();
        assert_eq!(game(10).proton_dir, ge.to_string_lossy());
        assert_eq!(game(20).proton_dir, "");
        assert_eq!(game(30).proton_dir, recorded.to_string_lossy());
        assert_eq!(game(10).library_path, second.to_string_lossy());
        assert_eq!(game(10).steam_root, root.to_string_lossy());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn discovery_reports_a_corrupt_config_vdf_and_caps_prefix_warnings() {
        let root = temp_dir("discover_warnings");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("config").join("config.vdf"), "\"broken\"\n{\n").unwrap();
        // No library list: the root is the default library.
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();

        // More unreadable (non-UTF-8) prefixes than the warning cap.
        let unresolvable = MAX_WARNINGS as u32 + 5;
        for app_id in 1..=unresolvable {
            install_game(&root, app_id, &format!("Game {app_id:02}"));
            write_config_info(&root, app_id, &[0xff, 0xfe]);
        }

        let discovery = discover_games_in(root.clone());
        assert_eq!(discovery.games.len(), unresolvable as usize);
        assert!(discovery.games.iter().all(|g| g.proton_dir.is_empty()));
        // The library list and config.vdf warnings, then prefixes up to the cap.
        assert_eq!(
            discovery.warnings.len(),
            MAX_WARNINGS,
            "{:?}",
            discovery.warnings
        );
        assert!(discovery.warnings[0].contains("library list"));
        assert!(discovery.warnings[1].contains("config.vdf"));
        assert!(discovery.warnings[2].contains("proton dir for app 1:"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovery_reports_an_unreadable_manifest_directory() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = temp_dir("discover_unreadable");
        let steamapps = root.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(steamapps.join("libraryfolders.vdf"), "x").unwrap();
        std::fs::set_permissions(&steamapps, std::fs::Permissions::from_mode(0o300)).unwrap();

        // Root ignores permissions, so there is nothing to observe then.
        let readable = std::fs::read_dir(&steamapps).is_ok();
        let discovery = discover_games_in(root.clone());
        std::fs::set_permissions(&steamapps, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !readable {
            assert!(discovery.games.is_empty());
            assert!(
                discovery
                    .warnings
                    .iter()
                    .any(|w| w.contains("Failed to read manifests")),
                "{:?}",
                discovery.warnings
            );
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_games_reports_a_missing_steam() {
        if !is_child() {
            return run_in_child(
                "steam::tests::discover_games_reports_a_missing_steam",
                &[],
                &["HOME"],
            );
        }
        assert!(matches!(discover_games(), Err(SteamError::SteamNotFound)));
    }

    #[test]
    fn discover_games_scans_the_steam_root_under_home() {
        // Fixed (pid-free) so the child resolves the same directory.
        let home = std::env::temp_dir().join("protonctx_test_discover_home");
        if !is_child() {
            let root = home.join(".local/share/Steam");
            std::fs::create_dir_all(root.join("steamapps")).unwrap();
            std::fs::write(root.join("steamapps").join("libraryfolders.vdf"), "x").unwrap();
            install_game(&root, 7, "Seven");
            run_in_child(
                "steam::tests::discover_games_scans_the_steam_root_under_home",
                &[("HOME", home.as_os_str())],
                &[],
            );
            std::fs::remove_dir_all(&home).ok();
            return;
        }
        let discovery = discover_games().unwrap();
        assert_eq!(discovery.games.len(), 1);
        assert_eq!(discovery.games[0].name, "Seven");
    }
}
