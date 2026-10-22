//! Marks dialog windows as transient dialogs of the main window on X11.
//!
//! iced 0.14 has no parent/child window setting, but on X11 the relationship
//! is plain window properties any client may set: `WM_TRANSIENT_FOR` names
//! the parent, which makes the window manager keep the dialog above it,
//! centre it on it, and leave it out of the taskbar, as Qt and GTK dialogs
//! are. The dialog is created unmapped, marked here through a separate
//! pure-Rust X11 connection, then shown, so the window manager sees the hints
//! when it first manages the window.

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
