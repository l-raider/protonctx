//! Marks dialog windows as transient dialogs of the main window on X11.
//!
//! Slint (through winit) has no parent/child window setting, but on X11 the
//! relationship is plain window properties any client may set:
//! `WM_TRANSIENT_FOR` names the parent, which makes the window manager keep
//! the dialog above it, centre it on it, and leave it out of the taskbar, as
//! Qt and GTK dialogs are. The dialog is marked here through a separate
//! pure-Rust X11 connection once its window exists, then remapped (see
//! `app::parent_when_mapped`), so the window manager sees the hints when it
//! manages the window.

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// Set the dialog hints on X window `dialog`, owned by X window `parent`:
/// transient for the parent, dialog type, and modal (which also skips the
/// taskbar and pager on window managers that do not infer it).
pub fn mark_dialog(dialog: u32, parent: u32) -> Result<(), String> {
    let (conn, _screen) = x11rb::connect(None).map_err(|error| error.to_string())?;
    let atom = |name: &str| -> Result<u32, String> {
        conn.intern_atom(false, name.as_bytes())
            .map_err(|error| error.to_string())?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|error| error.to_string())
    };
    let window_type = atom("_NET_WM_WINDOW_TYPE")?;
    let dialog_type = atom("_NET_WM_WINDOW_TYPE_DIALOG")?;
    let state = atom("_NET_WM_STATE")?;
    let states = [
        atom("_NET_WM_STATE_MODAL")?,
        atom("_NET_WM_STATE_SKIP_TASKBAR")?,
        atom("_NET_WM_STATE_SKIP_PAGER")?,
    ];

    let property = |property: u32, kind: AtomEnum, values: &[u32]| -> Result<(), String> {
        conn.change_property32(PropMode::REPLACE, dialog, property, kind, values)
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())
    };
    property(
        AtomEnum::WM_TRANSIENT_FOR.into(),
        AtomEnum::WINDOW,
        &[parent],
    )?;
    property(window_type, AtomEnum::ATOM, &[dialog_type])?;
    property(state, AtomEnum::ATOM, &states)?;
    conn.flush().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use crate::test_support::{is_child, run_in_child};

    /// Marking needs an X server; without one it reports the connection
    /// failure. (The property writes need a live server and window.)
    #[test]
    fn marking_without_an_x_server_reports_the_connection_error() {
        if !is_child() {
            return run_in_child(
                "x11_dialog::tests::marking_without_an_x_server_reports_the_connection_error",
                &[("DISPLAY", OsStr::new(":31999"))],
                &[],
            );
        }
        let error = super::mark_dialog(1, 2).unwrap_err();
        assert!(!error.is_empty());
    }

    /// A private Xvfb server on a free display, killed on drop.
    struct Xvfb(std::process::Child, String);

    impl Xvfb {
        /// `None` when Xvfb is not installed.
        fn start() -> Option<Self> {
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
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn a_marked_dialog_is_a_modal_transient_of_its_parent() {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};

        if !is_child() {
            let Some(server) = Xvfb::start() else {
                eprintln!("skipped: Xvfb is not installed");
                return;
            };
            return run_in_child(
                "x11_dialog::tests::a_marked_dialog_is_a_modal_transient_of_its_parent",
                &[("DISPLAY", OsStr::new(&server.1))],
                &[],
            );
        }
        let (conn, screen) = x11rb::connect(None).unwrap();
        let root = conn.setup().roots[screen].root;
        let window = || {
            let id = conn.generate_id().unwrap();
            conn.create_window(
                0,
                id,
                root,
                0,
                0,
                10,
                10,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
            id
        };
        let (parent, dialog) = (window(), window());

        super::mark_dialog(dialog, parent).unwrap();

        let atom = |name: &str| {
            conn.intern_atom(false, name.as_bytes())
                .unwrap()
                .reply()
                .unwrap()
                .atom
        };
        let values = |property: u32, kind: AtomEnum| -> Vec<u32> {
            conn.get_property(false, dialog, property, kind, 0, 16)
                .unwrap()
                .reply()
                .unwrap()
                .value32()
                .unwrap()
                .collect()
        };
        assert_eq!(
            values(AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::WINDOW),
            [parent]
        );
        assert_eq!(
            values(atom("_NET_WM_WINDOW_TYPE"), AtomEnum::ATOM),
            [atom("_NET_WM_WINDOW_TYPE_DIALOG")]
        );
        assert_eq!(
            values(atom("_NET_WM_STATE"), AtomEnum::ATOM),
            [
                atom("_NET_WM_STATE_MODAL"),
                atom("_NET_WM_STATE_SKIP_TASKBAR"),
                atom("_NET_WM_STATE_SKIP_PAGER"),
            ]
        );

        // A window that does not exist is reported, not panicked on.
        assert!(super::mark_dialog(0x00ff_fff0, parent).is_err());
    }
}
