//! Shared HOME and XDG base-directory resolution (config, Steam discovery,
//! Flatpak integration, launch-root fallback).

use std::path::PathBuf;

/// Return the home directory, if it can be determined.
///
/// `$HOME` is the single source of truth for every module that needs it, so the
/// precedence is identical across config, Steam discovery, Flatpak integration,
/// and the launcher.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolve an XDG base directory for the given environment variable, falling
/// back to the given `$HOME`-relative suffix when the variable is unset, empty,
/// or relative.
///
/// XDG base directories must be absolute paths per the spec; a relative value is
/// ignored in favour of the default.
pub fn xdg_dir(env_var: &str, home_fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(env_var) {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() && !dir.as_os_str().is_empty() {
            return Some(dir);
        }
    }
    home_dir().map(|home| home.join(home_fallback))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::PathBuf;

    use super::{home_dir, xdg_dir};
    use crate::test_support::{is_child, run_in_child};

    #[test]
    fn home_and_absolute_xdg_values_are_used() {
        if !is_child() {
            return run_in_child(
                "xdg::tests::home_and_absolute_xdg_values_are_used",
                &[
                    ("HOME", OsStr::new("/home/tester")),
                    ("PROTONCTX_XDG_ABS", OsStr::new("/srv/config")),
                    ("PROTONCTX_XDG_REL", OsStr::new("relative/config")),
                    ("PROTONCTX_XDG_EMPTY", OsStr::new("")),
                ],
                &["PROTONCTX_XDG_UNSET"],
            );
        }
        assert_eq!(home_dir(), Some(PathBuf::from("/home/tester")));
        assert_eq!(
            xdg_dir("PROTONCTX_XDG_ABS", ".config"),
            Some(PathBuf::from("/srv/config"))
        );
        // Relative, empty, and unset values fall back to the HOME suffix.
        for var in [
            "PROTONCTX_XDG_REL",
            "PROTONCTX_XDG_EMPTY",
            "PROTONCTX_XDG_UNSET",
        ] {
            assert_eq!(
                xdg_dir(var, ".config"),
                Some(PathBuf::from("/home/tester/.config")),
                "{var}"
            );
        }
    }

    #[test]
    fn an_empty_or_missing_home_resolves_nothing() {
        if !is_child() {
            run_in_child(
                "xdg::tests::an_empty_or_missing_home_resolves_nothing",
                &[("HOME", OsStr::new(""))],
                &["PROTONCTX_XDG_UNSET"],
            );
            return run_in_child(
                "xdg::tests::an_empty_or_missing_home_resolves_nothing",
                &[],
                &["HOME", "PROTONCTX_XDG_UNSET"],
            );
        }
        assert_eq!(home_dir(), None);
        assert_eq!(xdg_dir("PROTONCTX_XDG_UNSET", ".config"), None);
    }
}
