//! Parse `libraryfolders.vdf` to enumerate Steam library folders.

use std::path::{Path, PathBuf};

use steam_vdf_parser::parse_text;

use super::SteamError;

/// Return the absolute paths of every Steam library folder listed in `libraryfolders.vdf`.
///
/// `steam_root` is the Steam installation root (e.g. `~/.local/share/Steam`). The
/// canonical `libraryfolders.vdf` lives at `<steam_root>/config/libraryfolders.vdf` on
/// modern Steam, but historically it also appears directly under `<steam_root>/steamapps/`.
pub fn library_folders(steam_root: &Path) -> Result<Vec<PathBuf>, SteamError> {
    let config_vdf = steam_root.join("config").join("libraryfolders.vdf");
    let steamapps_vdf = steam_root.join("steamapps").join("libraryfolders.vdf");

    let path = if config_vdf.is_file() {
        config_vdf
    } else if steamapps_vdf.is_file() {
        steamapps_vdf
    } else {
        return Err(SteamError::Parse(
            "libraryfolders.vdf not found".to_string(),
        ));
    };

    let text = std::fs::read_to_string(&path)?;
    let vdf =
        parse_text(&text).map_err(|e| SteamError::Parse(format!("libraryfolders.vdf: {e}")))?;

    // The root key is "libraryfolders"; its value is the object keyed by library id.
    let Some(libraryfolders) = vdf.as_obj() else {
        return Err(SteamError::Parse(
            "libraryfolders.vdf: root value is not an object".to_string(),
        ));
    };

    let mut folders = Vec::new();
    // Each child is keyed by a numeric id ("0", "1", ...). The value is an object with a
    // "path" string. We intentionally read the id-less "path" per folder and preserve
    // all of them (including the default library).
    for (_id, value) in libraryfolders.iter() {
        let Some(folder_obj) = value.as_obj() else {
            continue;
        };
        if let Some(path_str) = folder_obj.get("path").and_then(|v| v.as_str()) {
            let p = PathBuf::from(path_str);
            if p.is_dir() {
                folders.push(p);
            }
        }
    }

    folders.sort();
    folders.dedup();
    Ok(folders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_libraryfolders_vdf() {
        let dir = std::env::temp_dir().join(format!("protonctx_test_lf_{}", std::process::id()));
        let lib_a = dir.join("Steam");
        let lib_b = dir.join("games");
        std::fs::create_dir_all(&lib_a).unwrap();
        std::fs::create_dir_all(&lib_b).unwrap();

        let vdf = format!(
            r#""libraryfolders"
{{
	"0"
	{{
		"path"		"{}"
		"apps"
		{{
			"274190"		"571899614"
		}}
	}}
	"1"
	{{
		"path"		"{}"
		"apps"
		{{
			"730"		"12345"
		}}
	}}
}}"#,
            lib_a.display(),
            lib_b.display()
        );

        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config").join("libraryfolders.vdf"), vdf).unwrap();

        let folders = library_folders(&dir).unwrap();
        assert_eq!(folders.len(), 2);
        assert!(folders.contains(&lib_a));
        assert!(folders.contains(&lib_b));

        std::fs::remove_dir_all(&dir).ok();
    }
}
