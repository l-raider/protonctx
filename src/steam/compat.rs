//! Read the per-app compatibility tool mapping from `config.vdf`, and resolve a tool's
//! install directory from its (internal) name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use steam_vdf_parser::parse_text;

use super::SteamError;

/// Resolve the install directory of a compatibility tool by its internal name.
///
/// Built-in tools (e.g. `"proton_experimental"`) live under `<steam_root>/steamapps/common/`
/// with the *display* name as directory name (e.g. `Proton - Experimental`); the internal
/// name is the lowercased, underscore-joined display name without spaces ("Proton
/// Experimental" → `proton_experimental`). The appmanifests carry this mapping
/// (`name` ↔ `installdir`), so we use them — see [`builtin_tool_dirs`]. Custom tools
/// (e.g. `"GE-Proton10-34"`) live under `<steam_root>/compatibilitytools.d/` and use the
/// internal name as their directory name, so the path check succeeds immediately.
///
/// Returns `None` when the tool is not installed or not a Proton-type tool (e.g. a
/// non-Proton compatibility layer like Boxtron), rather than an error — the caller falls
/// back to the prefix's recorded tool then.
///
/// `builtins` is the prebuilt [`builtin_tool_dirs`] map: callers resolving several
/// tools for the same `steam_root` (e.g. [`super::discover_games`]) build it once and
/// reuse it, avoiding a full re-read and VDF-parse of every appmanifest per call.
pub fn proton_dir_for_tool(
    steam_root: &Path,
    builtins: &HashMap<String, String>,
    tool: &str,
) -> Option<PathBuf> {
    if tool.is_empty() {
        return None;
    }

    let custom = steam_root.join("compatibilitytools.d").join(tool);
    if custom.is_dir() {
        return Some(custom);
    }

    resolve_builtin_dir(steam_root, builtins, tool)
}

/// Map of internal compat-tool name → install directory (under `steamapps/common/`)
/// for every installed built-in Proton tool, built from the `appmanifest_*.acf` files.
///
/// Reading and VDF-parsing every appmanifest is not cheap, so callers that resolve
/// several tools in one pass (e.g. [`super::discover_games`]) should build this map
/// once and reuse it via [`resolve_builtin_dir`] rather than calling
/// [`proton_dir_for_tool`] per tool — which would re-read the whole manifest
/// directory on every call (O(games × manifests)).
pub fn builtin_tool_dirs(steam_root: &Path) -> HashMap<String, String> {
    let steamapps = steam_root.join("steamapps");

    std::fs::read_dir(&steamapps)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                return None;
            }
            parse_builtin_appmanifest(&entry.path())
        })
        .collect()
}

/// Resolve a built-in tool's install directory from a prebuilt [`builtin_tool_dirs`]
/// map, verifying the directory actually exists under `steamapps/common/`.
pub fn resolve_builtin_dir(
    steam_root: &Path,
    builtins: &HashMap<String, String>,
    internal_name: &str,
) -> Option<PathBuf> {
    if !internal_name.starts_with("proton_") {
        return None;
    }

    let dir = steam_root
        .join("steamapps")
        .join("common")
        .join(builtins.get(internal_name)?);

    dir.is_dir().then_some(dir)
}

/// Parse a built-in tool's `appmanifest_<appid>.acf` into its internal name (`name`) and
/// install dir (`installdir`). Runtimes (e.g. `Steam Linux Runtime`) are filtered out here.
fn parse_builtin_appmanifest(path: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let vdf = parse_text(&text).ok()?;
    let app_state = vdf.as_obj()?;

    let name = app_state
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let install_dir = app_state
        .get("installdir")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // Only Proton tools (not runtimes, like "Steam Linux Runtime").
    if !name.starts_with("Proton") {
        return None;
    }

    // "Proton Experimental" → "proton_experimental"
    let internal = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .to_lowercase();

    Some((internal, install_dir))
}

/// Load the compatibility tool mapping from Steam's `config.vdf`.
///
/// Returns a map of Steam App ID (as a string, since the VDF keys are strings) to the
/// tool's internal name (e.g. `"proton_experimental"`). An empty map is returned when
/// `config.vdf` is missing or simply has no `CompatToolMapping` entries (which is normal
/// on a fresh Steam install — apps then use the global default); a genuinely unreadable or
/// unparsable file is reported as a `SteamError` for the caller to handle.
pub fn compat_tool_map(steam_root: &Path) -> Result<HashMap<String, String>, SteamError> {
    let config_vdf = steam_root.join("config").join("config.vdf");
    if !config_vdf.is_file() {
        return Ok(HashMap::new());
    }

    let text = std::fs::read_to_string(&config_vdf)?;
    let vdf = parse_text(&text).map_err(|e| SteamError::Parse(format!("config.vdf: {e}")))?;

    // Structure: root key "InstallConfigStore" → { Software → { Valve|valve → { Steam → { CompatToolMapping → { appid → { name } } } } } }.
    // `vdf.as_obj()` returns the object under the root key, so traversal starts at "Software".
    // The Valve key has been observed as both "Valve" and "valve" (see ProtonUp-Qt #226).
    let mapping = vdf
        .as_obj()
        .and_then(|root| root.get("Software").and_then(|v| v.as_obj()))
        .and_then(|software| {
            software
                .get("Valve")
                .or_else(|| software.get("valve"))
                .and_then(|v| v.as_obj())
        })
        .and_then(|valve| valve.get("Steam").and_then(|v| v.as_obj()))
        .and_then(|steam| steam.get("CompatToolMapping").and_then(|v| v.as_obj()));

    let Some(mapping) = mapping else {
        return Ok(HashMap::new());
    };

    let mut map = HashMap::new();
    for (app_id, value) in mapping.iter() {
        let Some(tool_obj) = value.as_obj() else {
            continue;
        };
        if let Some(name) = tool_obj.get("name").and_then(|v| v.as_str()) {
            map.insert(app_id.to_string(), name.to_string());
        }
    }

    Ok(map)
}

/// Resolve an app's compatibility tool name, falling back to Steam's global
/// default tool when the app has no explicit per-app override.
///
/// Steam records the globally-selected default tool under the `"0"` key of
/// `config.vdf`'s `CompatToolMapping` (the same map [`compat_tool_map`] returns), so a
/// game that inherits the Steam-wide default is reported with its actual tool rather
/// than an empty name — which would otherwise make the "Compatibility Tool" column
/// wrong and force `proton_dir` onto the (documented-stale) `config_info` fallback.
pub fn compat_tool_for_app(map: &HashMap<String, String>, app_id: u32) -> String {
    map.get(&app_id.to_string())
        .or_else(|| map.get("0"))
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compat_tool_mapping() {
        let vdf = r#""InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"CompatToolMapping"
				{
					"274190"
					{
						"name"		"proton_experimental"
						"priority"		"250"
					}
					"0"
					{
						"name"		"proton_hotfix"
					}
				}
			}
		}
	}
}"#;

        let dir =
            std::env::temp_dir().join(format!("protonctx_test_compat_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config").join("config.vdf"), vdf).unwrap();

        let map = compat_tool_map(&dir).unwrap();
        assert_eq!(
            map.get("274190").map(String::as_str),
            Some("proton_experimental")
        );
        assert_eq!(map.get("0").map(String::as_str), Some("proton_hotfix"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_when_no_mapping() {
        let vdf = r#""InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"Other"		"thing"
			}
		}
	}
}"#;

        let dir = std::env::temp_dir().join(format!(
            "protonctx_test_compat_empty_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config").join("config.vdf"), vdf).unwrap();

        let map = compat_tool_map(&dir).unwrap();
        assert!(map.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- proton_dir_for_tool ---

    /// Write an `appmanifest_<appid>.acf` for a built-in tool inside `steamapps`.
    fn write_builtin_appmanifest(
        steamapps: &std::path::Path,
        name: &str,
        install_dir: &str,
        appid: u32,
    ) {
        std::fs::create_dir_all(steamapps).unwrap();
        std::fs::write(
            steamapps.join(format!("appmanifest_{appid}.acf")),
            format!(
                "\"AppState\"\n{{\n  \"appid\"\t\t\"{appid}\"\n  \"name\"\t\t\"{name}\"\n  \"installdir\"\t\t\"{install_dir}\"\n}}\n"
            ),
        )
        .unwrap();
    }

    /// Create `steam_root/steamapps/common/<dir>` for a built-in tool, matching the real
    /// layout where appmanifests sit in `steamapps/` and install dirs in `steamapps/common/`.
    fn mk_common(steam_root: &std::path::Path, subdir: &str) -> std::path::PathBuf {
        let common = steam_root.join("steamapps").join("common");
        let dir = common.join(subdir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_custom_ge_proton() {
        let root =
            std::env::temp_dir().join(format!("protonctx_test_tool_custom_{}", std::process::id()));
        let ge = root.join("compatibilitytools.d").join("GE-Proton10-34");
        std::fs::create_dir_all(&ge).unwrap();

        let resolved = proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "GE-Proton10-34");
        assert_eq!(resolved, Some(ge));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolves_builtin_experimental() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_tool_builtin_{}",
            std::process::id()
        ));
        let steamapps = root.join("steamapps");
        // Mirrors the real install: display name "Proton Experimental" → install
        // dir "Proton - Experimental" (appid 1493710). Appmanifest lives in
        // `steamapps/`, install dir in `steamapps/common/`.
        write_builtin_appmanifest(
            &steamapps,
            "Proton Experimental",
            "Proton - Experimental",
            1493710,
        );
        let pe = mk_common(&root, "Proton - Experimental");

        let resolved = proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "proton_experimental");
        assert_eq!(resolved, Some(pe));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn builtin_ignores_linux_runtime() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_tool_runtime_{}",
            std::process::id()
        ));
        let steamapps = root.join("steamapps");
        // A runtime's manifest name starts with "Steam Linux Runtime", not "Proton".
        write_builtin_appmanifest(
            &steamapps,
            "Steam Linux Runtime 4.0",
            "SteamLinuxRuntime_4",
            4183110,
        );
        mk_common(&root, "SteamLinuxRuntime_4");

        // Runtimes must never be resolved as Proton tools.
        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "steam_linux_runtime_4"), None);
        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "proton_experimental"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn none_when_tool_missing() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_tool_missing_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).ok();

        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "GE-Proton10-34"), None);
        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "proton_experimental"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn none_for_empty_tool() {
        let root =
            std::env::temp_dir().join(format!("protonctx_test_tool_empty_{}", std::process::id()));
        std::fs::create_dir_all(&root).ok();

        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), ""), None);
        assert_eq!(proton_dir_for_tool(&root, &builtin_tool_dirs(&root), "default"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    // --- builtin_tool_dirs / resolve_builtin_dir (cached lookup) ---

    #[test]
    fn builtin_map_parses_all_tools_once() {        let root = std::env::temp_dir().join(format!(
            "protonctx_test_builtin_map_{}",
            std::process::id()
        ));
        let steamapps = root.join("steamapps");
        write_builtin_appmanifest(
            &steamapps,
            "Proton Experimental",
            "Proton - Experimental",
            1493710,
        );
        write_builtin_appmanifest(&steamapps, "Proton Hotfix", "Proton - Hotfix", 961940);
        // A runtime must be filtered out of the map.
        write_builtin_appmanifest(
            &steamapps,
            "Steam Linux Runtime 4.0",
            "SteamLinuxRuntime_4",
            4183110,
        );

        let map = builtin_tool_dirs(&root);
        assert_eq!(
            map.get("proton_experimental").map(String::as_str),
            Some("Proton - Experimental")
        );
        assert_eq!(
            map.get("proton_hotfix").map(String::as_str),
            Some("Proton - Hotfix")
        );
        assert!(!map.contains_key("steam_linux_runtime_4"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reused_map_resolves_repeatedly() {
        // The point of the cached map: resolving many tools against one map gives
        // the same answer every time (and does not depend on the manifest files
        // still being present after the map was built).
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_builtin_cached_{}",
            std::process::id()
        ));
        let steamapps = root.join("steamapps");
        write_builtin_appmanifest(
            &steamapps,
            "Proton Experimental",
            "Proton - Experimental",
            1493710,
        );
        write_builtin_appmanifest(&steamapps, "Proton Hotfix", "Proton - Hotfix", 961940);
        let pe = mk_common(&root, "Proton - Experimental");
        let ph = mk_common(&root, "Proton - Hotfix");

        let map = builtin_tool_dirs(&root);
        // Reuse the same map for several lookups (the discovery-loop pattern).
        assert_eq!(
            proton_dir_for_tool(&root, &map, "proton_experimental"),
            Some(pe.clone())
        );
        assert_eq!(
            proton_dir_for_tool(&root, &map, "proton_hotfix"),
            Some(ph)
        );
        assert_eq!(
            proton_dir_for_tool(&root, &map, "proton_experimental"),
            Some(pe)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cached_lookup_still_resolves_custom_tools() {
        // Custom tools live under compatibilitytools.d and must resolve from the
        // cached path too (the map only covers built-ins).
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_builtin_custom_{}",
            std::process::id()
        ));
        let ge = root.join("compatibilitytools.d").join("GE-Proton10-34");
        std::fs::create_dir_all(&ge).unwrap();

        let map = builtin_tool_dirs(&root);
        assert_eq!(
            proton_dir_for_tool(&root, &map, "GE-Proton10-34"),
            Some(ge)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cached_lookup_ignores_non_proton_builtin() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_builtin_nonproton_{}",
            std::process::id()
        ));
        let steamapps = root.join("steamapps");
        write_builtin_appmanifest(
            &steamapps,
            "Steam Linux Runtime 4.0",
            "SteamLinuxRuntime_4",
            4183110,
        );
        mk_common(&root, "SteamLinuxRuntime_4");

        let map = builtin_tool_dirs(&root);
        assert_eq!(
            proton_dir_for_tool(&root, &map, "steam_linux_runtime_4"),
            None
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn compat_tool_falls_back_to_global_default() {
        let mut map = HashMap::new();
        map.insert("0".to_string(), "proton_experimental".to_string());

        assert_eq!(compat_tool_for_app(&map, 274190), "proton_experimental");
    }

    #[test]
    fn compat_tool_prefers_per_app_override() {
        let mut map = HashMap::new();
        map.insert("0".to_string(), "proton_experimental".to_string());
        map.insert("274190".to_string(), "proton_hotfix".to_string());

        assert_eq!(compat_tool_for_app(&map, 274190), "proton_hotfix");
        assert_eq!(compat_tool_for_app(&map, 730), "proton_experimental");
    }

    #[test]
    fn compat_tool_empty_when_no_mapping() {
        let map: HashMap<String, String> = HashMap::new();
        assert_eq!(compat_tool_for_app(&map, 274190), "");
    }
}
