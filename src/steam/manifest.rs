//! Parse `appmanifest_<id>.acf` files to enumerate installed Steam apps.

use std::path::Path;

use steam_vdf_parser::parse_text;

use super::SteamError;
use super::paths;

/// A single installed Steam app, as described by an `appmanifest` file.
#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub app_id: u32,
    pub name: String,
    /// The value of the `installdir` key (directory name under `steamapps/common`).
    pub install_dir: Option<String>,
}

/// The fields both manifest readers need, from a single VDF parse.
pub(super) struct AppStateFields {
    pub app_id: Option<u32>,
    pub name: String,
    pub install_dir: Option<String>,
}

/// Read and VDF-parse one `appmanifest_*.acf`; `None` when unreadable/corrupt.
pub(super) fn read_app_state(path: &Path) -> Option<AppStateFields> {
    let text = std::fs::read_to_string(path).ok()?;
    let vdf = parse_text(&text).ok()?;
    // The root key is "AppState"; its value is the object holding the app fields.
    let app_state = vdf.as_obj()?;

    Some(AppStateFields {
        app_id: app_state
            .get("appid")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        name: app_state
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        install_dir: app_state
            .get("installdir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// Whether `path`'s file name is an `appmanifest_*.acf` manifest.
pub(super) fn is_appmanifest(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
}

/// Read every `appmanifest_*.acf` in `<library>/steamapps/` and return the apps it
/// describes. Missing or malformed manifests are skipped (an app list should be
/// best-effort rather than fail wholesale).
pub fn installed_apps(library: &Path) -> Result<Vec<InstalledApp>, SteamError> {
    let steamapps = paths::steamapps_dir(library);
    if !steamapps.is_dir() {
        return Ok(Vec::new());
    }

    let mut apps = Vec::new();
    let entries = std::fs::read_dir(&steamapps)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !is_appmanifest(&path) {
            continue;
        }

        if let Some(app) = parse_manifest(&path) {
            apps.push(app);
        }
    }

    apps.sort_by_key(|a| a.app_id);
    Ok(apps)
}

/// Parse a single `appmanifest_<id>.acf` file into an [`InstalledApp`].
fn parse_manifest(path: &Path) -> Option<InstalledApp> {
    let state = read_app_state(path)?;
    Some(InstalledApp {
        app_id: state.app_id?,
        name: state.name,
        install_dir: state.install_dir,
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

        let dir = crate::test_support::temp_dir("manifest");
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
        let dir = crate::test_support::temp_dir("manifest_skip");
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(steamapps.join("libraryfolders.vdf"), "x").unwrap();

        let apps = installed_apps(&dir).unwrap();
        assert!(apps.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
