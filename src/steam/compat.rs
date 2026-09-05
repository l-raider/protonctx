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
/// (`name` ↔ `installdir`), so we use them — see [`manifest_for_builtin`]. Custom tools
/// (e.g. `"GE-Proton10-34"`) live under `<steam_root>/compatibilitytools.d/` and use the
/// internal name as their directory name, so the path check succeeds immediately.
///
/// Returns `None` when the tool is not installed or not a Proton-type tool (e.g. a
/// non-Proton compatibility layer like Boxtron), rather than an error — the caller falls
/// back to the prefix's recorded tool then.
pub fn proton_dir_for_tool(steam_root: &Path, tool: &str) -> Option<PathBuf> {
    if tool.is_empty() {
        return None;
    }

    let custom = steam_root.join("compatibilitytools.d").join(tool);
    if custom.is_dir() {
        return Some(custom);
    }

    let common = steam_root.join("steamapps").join("common");
    let builtin = manifest_for_builtin(&common, tool)?;
    let dir = common.join(builtin);
    if dir.is_dir() { Some(dir) } else { None }
}

/// Find an installed built-in tool matching an internal name like `"proton_experimental"`.
///
/// This mirrors Steam's own naming scheme: an internal compat name is the
/// whitespace-stripped, underscore-joined lowercase form of the tool's `appmanifest`
/// name (e.g. `Proton Experimental` → `proton_experimental`, `Proton Hotfix` →
/// `proton_hotfix`). The `name` field is authoritative for matching; the `installdir`
/// field is the actual directory under `steamapps/common/`. Runtimes are excluded
/// because their appmanifest name never starts with `"proton"`.
fn manifest_for_builtin(common: &Path, internal_name: &str) -> Option<String> {
    if !internal_name.starts_with("proton_") {
        return None;
    }

    // The `appmanifest_*.acf` files live in `steamapps/`, one level above `common/`.
    let steamapps = common.parent()?;

    // Build a map of internal name → install dir from the installed appmanifests.
    let installed: HashMap<String, String> = std::fs::read_dir(steamapps)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                return None;
            }
            parse_builtin_appmanifest(&entry.path())
        })
        .collect();

    installed.get(internal_name).cloned()
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

        let resolved = proton_dir_for_tool(&root, "GE-Proton10-34");
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
        let common = steamapps.join("common");
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

        let resolved = proton_dir_for_tool(&root, "proton_experimental");
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
        assert_eq!(proton_dir_for_tool(&root, "steam_linux_runtime_4"), None);
        assert_eq!(proton_dir_for_tool(&root, "proton_experimental"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn none_when_tool_missing() {
        let root = std::env::temp_dir().join(format!(
            "protonctx_test_tool_missing_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).ok();

        assert_eq!(proton_dir_for_tool(&root, "GE-Proton10-34"), None);
        assert_eq!(proton_dir_for_tool(&root, "proton_experimental"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn none_for_empty_tool() {
        let root =
            std::env::temp_dir().join(format!("protonctx_test_tool_empty_{}", std::process::id()));
        std::fs::create_dir_all(&root).ok();

        assert_eq!(proton_dir_for_tool(&root, ""), None);
        assert_eq!(proton_dir_for_tool(&root, "default"), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
