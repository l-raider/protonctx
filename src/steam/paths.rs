//! Canonical Steam directory-layout helpers. Every module that touches
//! `steamapps` paths goes through this module.

use std::path::{Path, PathBuf};

pub const STEAMAPPS: &str = "steamapps";
pub const COMMON: &str = "common";
pub const COMPATDATA: &str = "compatdata";
pub const SHADERCACHE: &str = "shadercache";
pub const COMPATIBILITYTOOLS_D: &str = "compatibilitytools.d";
pub const LIBRARYFOLDERS_VDF: &str = "libraryfolders.vdf";

/// The `steamapps` data directory under a Steam library root.
pub fn steamapps_dir(root: &Path) -> PathBuf {
    root.join(STEAMAPPS)
}

/// The compatdata (Proton prefix) directory for an app:
/// `<library>/steamapps/compatdata/<app_id>`.
pub fn app_compatdata_dir(library: &Path, app_id: u32) -> PathBuf {
    steamapps_dir(library)
        .join(COMPATDATA)
        .join(app_id.to_string())
}

/// The shader-cache directory for an app:
/// `<library>/steamapps/shadercache/<app_id>`.
pub fn app_shadercache_dir(library: &Path, app_id: u32) -> PathBuf {
    steamapps_dir(library)
        .join(SHADERCACHE)
        .join(app_id.to_string())
}

/// The install directory for an app: `<library>/steamapps/common/<install_dir>`.
pub fn app_common_dir(library: &Path, install_dir: &str) -> PathBuf {
    steamapps_dir(library).join(COMMON).join(install_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_canonical_app_paths() {
        let library = Path::new("/lib");

        assert_eq!(steamapps_dir(library), PathBuf::from("/lib/steamapps"));
        assert_eq!(
            app_compatdata_dir(library, 274190),
            PathBuf::from("/lib/steamapps/compatdata/274190")
        );
        assert_eq!(
            app_shadercache_dir(library, 274190),
            PathBuf::from("/lib/steamapps/shadercache/274190")
        );
        assert_eq!(
            app_common_dir(library, "Broforce"),
            PathBuf::from("/lib/steamapps/common/Broforce")
        );
    }
}
