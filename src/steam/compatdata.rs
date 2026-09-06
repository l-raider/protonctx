//! Resolve the Proton directory a game's prefix uses, from `compatdata/<appid>/config_info`.
//!
//! The `config_info` file (created by Steam when a prefix is first created) contains the
//! *authoritative* path of the Proton tool for that prefix, regardless of whether it is a
//! built-in tool under `steamapps/common/` or a custom tool under `compatibilitytools.d/`.
//! Its layout is line-based:
//!
//! ```text
//! 11.0-100
//! /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/files/share/fonts/
//! /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/files/lib/
//! /home/user/.local/share/Steam
//! 1788403030.0
//! ...
//! /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/files/share/default_pfx/
//! ...
//! ```
//!
//! The line ending in `/files/share/default_pfx/` identifies the tool's `dist` directory;
//! stripping that suffix yields the Proton directory itself.

use std::path::{Path, PathBuf};

use super::SteamError;

/// Suffix that marks the `default_pfx` line inside a Proton `dist` directory.
const DEFAULT_PFX_SUFFIX: &str = "/files/share/default_pfx/";

/// Resolve the Proton directory for the given app's prefix, or `None` if the prefix has not
/// been created yet (the game has never been run under Proton).
///
/// `library` is the Steam library the game is installed in: `config_info` lives under the
/// library's `steamapps/compatdata/<appid>/` (Steam creates the prefix next to the game
/// install, so a game on a secondary library keeps its prefix there, not under the root).
pub fn proton_dir_for(library: &Path, app_id: u32) -> Result<Option<PathBuf>, SteamError> {
    let Some(lines) = read_lines(library, app_id)? else {
        return Ok(None);
    };

    for line in lines {
        if let Some(idx) = line.rfind(DEFAULT_PFX_SUFFIX) {
            let proton_dir = &line[..idx];
            let path = Path::new(proton_dir);
            if path.is_dir() {
                return Ok(Some(path.to_path_buf()));
            }
        }
    }

    Ok(None)
}

/// Read the `config_info` file for an app's compatdata directory, if it exists.
///
/// A missing file returns `Ok(None)` (the prefix simply hasn't been created yet),
/// while a genuine I/O failure (e.g. permission denied) is propagated as an error
/// rather than silently swallowed.
fn read_lines(library: &Path, app_id: u32) -> Result<Option<Vec<String>>, SteamError> {
    let config_info = library
        .join("steamapps")
        .join("compatdata")
        .join(app_id.to_string())
        .join("config_info");

    if !config_info.is_file() {
        return Ok(None);
    }

    let text = std::fs::read_to_string(config_info)?;
    Ok(Some(text.lines().map(str::to_string).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config_info(dir: &Path, app_id: u32, content: &str) {
        let compatdata = dir
            .join("steamapps")
            .join("compatdata")
            .join(app_id.to_string());
        std::fs::create_dir_all(&compatdata).unwrap();
        std::fs::write(compatdata.join("config_info"), content).unwrap();
    }

    #[test]
    fn resolves_builtin_proton_dir() {
        let dir = std::env::temp_dir().join(format!("protonctx_test_cd_{}", std::process::id()));
        let proton = dir
            .join("steamapps")
            .join("common")
            .join("Proton - Experimental");
        std::fs::create_dir_all(&proton).unwrap();

        let content = format!(
            "11.0-100\n{0}/files/share/fonts/\n{0}/files/lib/\n{1}\n1788403030.0\n{0}/files/share/default_pfx/\n",
            proton.display(),
            dir.display()
        );
        write_config_info(&dir, 274190, &content);

        let resolved = proton_dir_for(&dir, 274190).unwrap().unwrap();
        assert_eq!(resolved, proton);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn none_when_no_prefix() {
        let dir =
            std::env::temp_dir().join(format!("protonctx_test_cd_none_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(proton_dir_for(&dir, 999999).unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
