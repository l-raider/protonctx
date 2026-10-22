//! Resolve and delete a game's Steam shader cache directory.
//!
//! Steam stores per-app shader caches under the *library* the game is installed
//! in, at `<library>/steamapps/shadercache/<app_id>`. The directory holds the
//! vendor shader pipeline caches (e.g. `fozpipelinesv6/`) that a game builds up
//! as it runs; emptying it forces the caches to be regenerated.
//!
//! The library root is not hardcoded: it comes from `libraryfolders.vdf` (via
//! [`super::libraryfolders`]), so a game installed on a secondary library
//! resolves to that library's `shadercache`, not the Steam root's.
//!
//! No VDF records a shader-cache path directly, so the path is *derived* from
//! the library root rather than read from a config file.

use std::path::{Path, PathBuf};

use super::paths;

/// The shader-cache directory for an app: `<library>/steamapps/shadercache/<app_id>`.
///
/// `library` is the Steam library root the game is installed in (the directory
/// that *contains* `steamapps/`), matching the convention used by
/// [`super::compatdata::proton_dir_for`]. The returned path is not guaranteed to
/// exist: a game that has never run under Proton has no cache yet.
pub fn shader_cache_dir_for(library: &Path, app_id: u32) -> PathBuf {
    paths::app_shadercache_dir(library, app_id)
}

/// Remove an app's shader-cache directory.
///
/// Deletes the entire `<library>/steamapps/shadercache/<app_id>` tree; Steam
/// recreates the directory on the next launch. Returns `Ok(true)` when the
/// directory existed and was removed, and `Ok(false)` when there was nothing to
/// delete (the game has no cache yet).
///
/// # Safety guard
///
/// Before deleting, the path is verified to be a direct child of a
/// `steamapps/shadercache` directory. This prevents a malformed or hostile
/// `library_path` from ever causing a recursive delete outside the expected
/// tree: if the guard fails, the call returns an [`std::io::Error`] instead of
/// touching the filesystem. Relative paths are rejected outright, so the
/// derived path can never be resolved against the process CWD.
///
/// # Symlinks
///
/// The cache path itself is never resolved. A symlinked *library* (which
/// `libraryfolders.vdf` may list) needs no special handling: the kernel follows
/// symlinked parent components while deleting. A symlinked *cache directory*
/// (people link `shadercache/<app_id>` to another disk) is removed as a link
/// only, and its target is left untouched: the target was never checked by the
/// guard, so deleting through the link could reach any directory. With the
/// link gone Steam recreates a real cache directory, which is what "delete the
/// cache" means to it.
pub fn delete_shader_cache(library: &Path, app_id: u32) -> std::io::Result<bool> {
    let dir = shader_cache_dir_for(library, app_id);

    // Reject anything that is not `<...>/steamapps/shadercache/<app_id>`. This is
    // the single guard that keeps a bad `library_path` from turning into a
    // recursive delete somewhere unexpected.
    if !is_shader_cache_dir(&dir, app_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to delete unexpected path: {}", dir.display()),
        ));
    }

    match CacheLeaf::of(&dir)? {
        CacheLeaf::Missing | CacheLeaf::Other => Ok(false),
        CacheLeaf::Link => std::fs::remove_file(&dir).map(|()| true),
        CacheLeaf::Dir => std::fs::remove_dir_all(&dir).map(|()| true),
    }
}

/// What sits at a shader-cache path, read without following a symlink at the
/// leaf (see [`delete_shader_cache`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheLeaf {
    /// Nothing: the game has no cache yet.
    Missing,
    /// A symlink, dangling or not; only the link itself may be removed.
    Link,
    /// A real directory.
    Dir,
    /// A regular file or other non-directory; left alone.
    Other,
}

impl CacheLeaf {
    fn of(dir: &Path) -> std::io::Result<Self> {
        let file_type = match std::fs::symlink_metadata(dir) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::Missing);
            }
            Err(error) => return Err(error),
        };
        Ok(if file_type.is_symlink() {
            Self::Link
        } else if file_type.is_dir() {
            Self::Dir
        } else {
            Self::Other
        })
    }
}

/// Whether `dir` has the exact shape `<...>/steamapps/shadercache/<app_id>`.
///
/// The check is structural (file names only, no filesystem access) so it also
/// rejects a path that does not exist yet. It is absolute-only: a relative
/// `library_path` would otherwise match the shape while resolving against the
/// process CWD, letting deletion escape the configured library.
fn is_shader_cache_dir(dir: &Path, app_id: u32) -> bool {
    if !dir.is_absolute() {
        return false;
    }

    let app = dir.file_name().and_then(|n| n.to_str());
    if app != Some(app_id.to_string().as_str()) {
        return false;
    }

    let Some(cache) = dir.parent() else {
        return false;
    };
    if cache.file_name().and_then(|n| n.to_str()) != Some(paths::SHADERCACHE) {
        return false;
    }

    let Some(steamapps) = cache.parent() else {
        return false;
    };
    steamapps.file_name().and_then(|n| n.to_str()) == Some(paths::STEAMAPPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_shader_cache_path() {
        let library = Path::new("/home/u/.local/share/Steam");
        assert_eq!(
            shader_cache_dir_for(library, 274190),
            PathBuf::from("/home/u/.local/share/Steam/steamapps/shadercache/274190")
        );
    }

    #[test]
    fn guard_accepts_expected_shape() {
        let dir = Path::new("/lib/steamapps/shadercache/274190");
        assert!(is_shader_cache_dir(dir, 274190));
        // A different app id must not match.
        assert!(!is_shader_cache_dir(dir, 123));
    }

    #[test]
    fn guard_rejects_unexpected_shapes() {
        // Missing the `shadercache` component.
        assert!(!is_shader_cache_dir(
            Path::new("/lib/steamapps/274190"),
            274190
        ));
        // Missing the `steamapps` component.
        assert!(!is_shader_cache_dir(
            Path::new("/lib/shadercache/274190"),
            274190
        ));
        // Extra depth below the app id.
        assert!(!is_shader_cache_dir(
            Path::new("/lib/steamapps/shadercache/274190/pfx"),
            274190
        ));
        // Root-relative path with too few components.
        assert!(!is_shader_cache_dir(Path::new("/274190"), 274190));
    }

    #[test]
    fn guard_rejects_relative_paths() {
        // A relative `library_path` would resolve against the process CWD and
        // let a recursive delete escape the configured library.
        assert!(!is_shader_cache_dir(
            Path::new("steamapps/shadercache/274190"),
            274190
        ));
        assert!(!is_shader_cache_dir(
            Path::new("../../lib/steamapps/shadercache/274190"),
            274190
        ));

        // `delete_shader_cache` derives the path itself, so a relative library
        // is refused before any filesystem access.
        let err = delete_shader_cache(Path::new("relative-lib"), 274190).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn delete_removes_existing_cache() {
        let root = crate::test_support::temp_dir("shadercache");
        let cache = shader_cache_dir_for(&root, 274190);
        std::fs::create_dir_all(cache.join("fozpipelinesv6")).unwrap();

        assert!(delete_shader_cache(&root, 274190).unwrap());
        assert!(!cache.exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_removes_a_symlinked_cache_but_not_its_target() {
        // People move shader caches to another disk with a symlink. Deleting
        // must remove the link only: the target was never checked by the guard.
        let root = crate::test_support::temp_dir("shadercache_symlink");
        let target = root.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), "keep").unwrap();
        let library = root.join("lib");
        let cache = shader_cache_dir_for(&library, 274190);
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &cache).unwrap();

        assert!(delete_shader_cache(&library, 274190).unwrap());
        assert!(
            target.join("keep.txt").is_file(),
            "the link target survives"
        );
        assert!(
            std::fs::symlink_metadata(&cache).is_err(),
            "the link itself is gone"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_removes_a_dangling_symlinked_cache() {
        let root = crate::test_support::temp_dir("shadercache_dangling");
        let cache = shader_cache_dir_for(&root, 274190);
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), &cache).unwrap();

        assert!(delete_shader_cache(&root, 274190).unwrap());
        assert!(std::fs::symlink_metadata(&cache).is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_leaves_a_regular_file_alone() {
        let root = crate::test_support::temp_dir("shadercache_file");
        let cache = shader_cache_dir_for(&root, 274190);
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, "not a directory").unwrap();

        assert!(!delete_shader_cache(&root, 274190).unwrap());
        assert!(cache.is_file());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_reports_nothing_to_do_when_absent() {
        let root = crate::test_support::temp_dir("shadercache_absent");
        std::fs::create_dir_all(&root).unwrap();

        assert!(!delete_shader_cache(&root, 274190).unwrap());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_refuses_unexpected_path() {
        // `delete_shader_cache` derives the path itself, so the guard is the
        // single place that can reject a malformed location. Exercise it
        // directly: a leaf that is not under `steamapps/shadercache` must be
        // refused rather than deleted.
        let root = crate::test_support::temp_dir("shadercache_guard");
        // `<root>/274190` is missing the `steamapps/shadercache` parents.
        assert!(!is_shader_cache_dir(&root.join("274190"), 274190));
    }
}
