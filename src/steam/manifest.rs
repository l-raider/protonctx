//! Parse `appmanifest_<id>.acf` files to enumerate installed Steam apps.

use std::path::Path;

use steam_vdf_parser::parse_text;

use super::SteamError;

/// A single installed Steam app, as described by an `appmanifest` file.
#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub app_id: u32,
    pub name: String,
    /// The value of the `installdir` key (directory name under `steamapps/common`).
    pub install_dir: Option<String>,
}

/// Read every `appmanifest_*.acf` in `<library>/steamapps/` and return the apps it
/// describes. Missing or malformed manifests are skipped (an app list should be
/// best-effort rather than fail wholesale).
pub fn installed_apps(library: &Path) -> Result<Vec<InstalledApp>, SteamError> {
    let steamapps = library.join("steamapps");
    if !steamapps.is_dir() {
        return Ok(Vec::new());
    }

    let mut apps = Vec::new();
    let entries = std::fs::read_dir(&steamapps)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
            continue;
        }

        let path = entry.path();
        if let Some(app) = parse_manifest(&path) {
            apps.push(app);
        }
    }

    apps.sort_by_key(|a| a.app_id);
    Ok(apps)
}

/// Parse a single `appmanifest_<id>.acf` file into an [`InstalledApp`].
fn parse_manifest(path: &Path) -> Option<InstalledApp> {
    let text = std::fs::read_to_string(path).ok()?;
    let vdf = parse_text(&text).ok()?;
    // The root key is "AppState"; its value is the object holding the app fields.
    let app_state = vdf.as_obj()?;

    let app_id = app_state
        .get("appid")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u32>().ok())?;

    let name = app_state
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let install_dir = app_state
        .get("installdir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Some(InstalledApp {
        app_id,
        name,
        install_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_broforce_manifest() {
        let acf = r#""AppState"
{
	"appid"		"274190"
	"name"		"Broforce"
	"installdir"		"Broforce"
}"#;

        let dir =
            std::env::temp_dir().join(format!("protonctx_test_manifest_{}", std::process::id()));
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(steamapps.join("appmanifest_274190.acf"), acf).unwrap();

        let apps = installed_apps(&dir).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].app_id, 274190);
        assert_eq!(apps[0].name, "Broforce");
        assert_eq!(apps[0].install_dir.as_deref(), Some("Broforce"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn skips_non_manifest_files() {
        let dir = std::env::temp_dir().join(format!(
            "protonctx_test_manifest_skip_{}",
            std::process::id()
        ));
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(steamapps.join("libraryfolders.vdf"), "x").unwrap();

        let apps = installed_apps(&dir).unwrap();
        assert!(apps.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
