//! Read the per-app compatibility tool mapping from `config.vdf`.

use std::collections::HashMap;
use std::path::Path;

use steam_vdf_parser::parse_text;

use super::SteamError;

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
}
