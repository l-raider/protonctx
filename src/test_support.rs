//! Test-only helpers shared across the crate's unit tests.

use crate::models::Game;

/// A fresh temp directory named for `tag` and this process.
///
/// Each test uses a unique `tag`, so parallel tests never share a directory.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("protonctx_test_{tag}_{}", std::process::id()))
}

/// A [`Game`] fixture with every field populated. The library doubles as the
/// Steam root, the common single-library layout.
pub fn game(
    name: &str,
    app_id: u32,
    compat_tool: &str,
    library_path: &str,
    proton_dir: &str,
) -> Game {
    Game {
        name: name.to_string(),
        app_id,
        compat_tool: compat_tool.to_string(),
        library_path: library_path.to_string(),
        steam_root: library_path.to_string(),
        proton_dir: proton_dir.to_string(),
    }
}

/// The common two-argument fixture: a known tool and library layout.
pub fn test_game(name: &str, app_id: u32) -> Game {
    game(
        name,
        app_id,
        "proton_experimental",
        "/lib",
        "/lib/steamapps/common/Proton - Experimental",
    )
}

/// Set in the environment of a test re-run by [`run_in_child`].
const CHILD_MARKER: &str = "PROTONCTX_TEST_CHILD";

/// Whether this test process is a child started by [`run_in_child`].
pub fn is_child() -> bool {
    std::env::var_os(CHILD_MARKER).is_some()
}

/// Re-run the single test `name` (its full path, e.g.
/// `"config::tests::round_trip"`) in a child of this test binary, with `set`
/// added to and `unset` removed from the environment, and panic with its
/// output unless it ran and passed.
///
/// Edition 2024 makes `std::env::set_var` unsafe (and racy across parallel
/// tests), so code that reads the environment is tested this way: the test
/// calls this when [`is_child`] is false and runs its body when it is true.
/// Coverage tools profile the child too.
pub fn run_in_child(name: &str, set: &[(&str, &std::ffi::OsStr)], unset: &[&str]) {
    let exe = std::env::current_exe().expect("the test binary path");
    let mut command = std::process::Command::new(exe);
    command
        .args([
            name,
            "--exact",
            "--nocapture",
            "--test-threads=1",
            "--include-ignored",
        ])
        .env(CHILD_MARKER, "1");
    for key in unset {
        command.env_remove(key);
    }
    for (key, value) in set {
        command.env(key, value);
    }
    let output = command.output().expect("the test binary re-runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child test {name} failed:\n{stdout}\n{stderr}"
    );
}

/// A private Xvfb server on a free display, killed on drop: for tests that
/// need a real X server (and the winit backend on it).
pub struct Xvfb(std::process::Child, String);

impl Xvfb {
    /// `None` when Xvfb is not installed.
    pub fn start() -> Option<Self> {
        use std::io::BufRead as _;
        let mut child = std::process::Command::new("Xvfb")
            .args(["-displayfd", "1", "-nolisten", "tcp"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take()?)
            .read_line(&mut line)
            .ok()?;
        Some(Xvfb(child, format!(":{}", line.trim())))
    }

    /// The `DISPLAY` value that reaches the server.
    pub fn display(&self) -> &str {
        &self.1
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
