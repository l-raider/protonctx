//! The compiled Slint UI and its setup: the backend (winit on X11 with the
//! software renderer), the window attributes every window gets, and copying
//! the design tokens into each window's `Theme` global.

use std::cell::Cell;

use slint::ComponentHandle;
use slint::winit_030::winit;

use crate::theme::AppTokens;

/// The code `slint-build` generates from `ui/app.slint`. It does not follow
/// the workspace's restriction lints.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::todo)]
mod generated {
    slint::include_modules!();
}

pub use generated::*;

thread_local! {
    /// Set while a dialog component is being instantiated: the window
    /// attributes hook runs then (before the title is applied), so this is how
    /// it tells dialogs from the main window.
    static CREATING_DIALOG: Cell<bool> = const { Cell::new(false) };
}

/// Instantiate a dialog component, marking its window as an X11 dialog from
/// creation on.
pub fn new_dialog<T>(
    create: impl FnOnce() -> Result<T, slint::PlatformError>,
) -> Result<T, slint::PlatformError> {
    CREATING_DIALOG.with(|flag| flag.set(true));
    let created = create();
    CREATING_DIALOG.with(|flag| flag.set(false));
    created
}

/// Whether an X server (XWayland on a Plasma Wayland session) is available,
/// and so whether the windows are X11 ones.
pub(crate) fn has_x11_display() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
}

/// Select the winit backend with the software renderer, on X11 whenever an X
/// display exists (dialogs can only be parented there; see `x11_dialog`),
/// native Wayland otherwise. Every window carries the desktop file's id as
/// its X11 class (so KWin resolves the icon), and dialogs get the dialog
/// window type.
pub fn select_backend() -> Result<(), slint::PlatformError> {
    let app_id = crate::flatpak::window_app_id();
    let mut selector = slint::BackendSelector::new()
        .backend_name("winit".to_string())
        .renderer_name("software".to_string());
    if has_x11_display() {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        let mut event_loop =
            winit::event_loop::EventLoop::<slint::winit_030::SlintEvent>::with_user_event();
        event_loop.with_x11();
        selector = selector.with_winit_event_loop_builder(event_loop);
    }
    selector
        .with_winit_window_attributes_hook(move |attributes| {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            // Opaque: Slint asks for a transparent window, which on X11 selects
            // an ARGB visual the software renderer's pixels show through.
            let attributes = attributes
                .with_transparent(false)
                .with_name(app_id.clone(), app_id.clone());
            if CREATING_DIALOG.with(Cell::get) {
                attributes
                    .with_x11_window_type(vec![WindowType::Dialog])
                    .with_resizable(false)
            } else {
                attributes
            }
        })
        .select()
}

/// Copy the design tokens into `component`'s `Theme` global. Globals are per
/// window component in Slint, so every window gets its own copy.
pub fn apply_theme<T>(component: &T, t: &AppTokens)
where
    T: ComponentHandle,
    for<'a> Theme<'a>: slint::Global<'a, T>,
{
    let g = component.global::<Theme>();
    let c = slint::Color::from;
    let over = |top: crate::theme::Color, base: crate::theme::Color| {
        let mix = |a: f32, b: f32| a * top.a + b * (1.0 - top.a);
        c(crate::theme::Color {
            r: mix(top.r, base.r),
            g: mix(top.g, base.g),
            b: mix(top.b, base.b),
            a: 1.0,
        })
    };

    g.set_background(c(t.background));
    g.set_foreground(c(t.foreground));
    g.set_muted(c(t.muted));
    g.set_header(c(t.header));
    g.set_header_foreground(c(t.header_foreground));
    g.set_list_background(c(t.list_background));
    g.set_alternate_row(c(t.alternate_row));
    g.set_list_foreground(c(t.list_foreground));
    g.set_border(c(t.border));
    g.set_grid(c(t.grid));
    g.set_muted_foreground(c(t.muted_foreground));

    g.set_input_border(c(t.input_border));
    g.set_input_background(c(t.input_background));
    g.set_button_foreground(c(t.button_foreground));
    g.set_primary(c(t.primary));
    g.set_primary_foreground(c(t.primary_foreground));
    g.set_ring(c(t.ring));
    g.set_accent(c(t.accent));
    g.set_hover_even(over(t.accent, t.list_background));
    g.set_hover_odd(over(t.accent, t.alternate_row));
    g.set_selection_hover(c(t.selection_hover));
    g.set_selection(c(t.selection));
    g.set_danger(c(t.danger));
    g.set_warning(c(t.warning));
    g.set_radius(t.radius);
    g.set_disabled_opacity(t.disabled_opacity);

    g.set_menu_background(c(t.menu_background));
    g.set_menu_foreground(c(t.menu_foreground));
    g.set_menu_hover(c(t.menu_hover));
    g.set_menu_hover_border(c(t.menu_hover_border));

    g.set_scrollbar_track(c(t.scrollbar_track));
    g.set_scrollbar_separator(c(t.scrollbar_separator));
    g.set_scrollbar_thumb(c(t.scrollbar_thumb));
    g.set_scrollbar_thumb_hover(c(t.scrollbar_thumb_hover));
    g.set_scrollbar_thumb_active(c(t.scrollbar_thumb_active));
    g.set_scrollbar_width(t.scrollbar_width);
    g.set_scroller_width(t.scroller_width);
    g.set_scrollbar_inset(t.scrollbar_inset);
    g.set_scrollbar_min_thumb(t.scrollbar_min_thumb);

    g.set_font_family(t.font_family.as_str().into());
    g.set_font_weight(i32::from(t.font_weight));
    g.set_font_size(t.font_size);
    g.set_mono_family(t.mono_family.as_str().into());
    g.set_mono_size(t.mono_size);
    g.set_reduce_motion(t.reduce_motion);
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{has_x11_display, new_dialog, select_backend};
    use crate::test_support::{is_child, run_in_child};

    #[test]
    fn an_x11_display_is_a_non_empty_display_variable() {
        if !is_child() {
            run_in_child(
                "ui::tests::an_x11_display_is_a_non_empty_display_variable",
                &[
                    ("DISPLAY", OsStr::new(":0")),
                    ("PROTONCTX_EXPECT", OsStr::new("1")),
                ],
                &[],
            );
            run_in_child(
                "ui::tests::an_x11_display_is_a_non_empty_display_variable",
                &[("DISPLAY", OsStr::new(""))],
                &["PROTONCTX_EXPECT"],
            );
            return run_in_child(
                "ui::tests::an_x11_display_is_a_non_empty_display_variable",
                &[],
                &["DISPLAY", "PROTONCTX_EXPECT"],
            );
        }
        assert_eq!(
            has_x11_display(),
            std::env::var_os("PROTONCTX_EXPECT").is_some()
        );
    }

    /// Without a reachable display the backend cannot start; selecting it
    /// reports that instead of panicking, on X11 and on Wayland alike.
    #[test]
    fn the_backend_fails_cleanly_without_a_display() {
        if !is_child() {
            let unset = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "FLATPAK_ID"];
            run_in_child(
                "ui::tests::the_backend_fails_cleanly_without_a_display",
                &[("DISPLAY", OsStr::new(":31999"))],
                &unset,
            );
            return run_in_child(
                "ui::tests::the_backend_fails_cleanly_without_a_display",
                &[("XDG_RUNTIME_DIR", OsStr::new("/nonexistent"))],
                &[&unset[..], &["DISPLAY"]].concat(),
            );
        }
        let selected = select_backend();
        // Creating a window is where the event loop meets the display.
        let window = selected.and_then(|()| super::MainWindow::new().map(drop));
        assert!(window.is_err());
    }

    #[test]
    fn new_dialog_passes_through_the_component_and_its_error() {
        let created = new_dialog(|| Ok::<_, slint::PlatformError>(7));
        assert_eq!(created.unwrap(), 7);
        let failed = new_dialog(|| Err::<(), _>(slint::PlatformError::NoPlatform));
        assert!(failed.is_err());
    }

    /// The About dialog grows with a large desktop font (on the real winit
    /// backend, under Xvfb), so its OK button stays in the window.
    #[test]
    fn the_about_dialog_grows_to_keep_its_button_in_the_window() {
        use slint::ComponentHandle as _;
        use slint::platform::{PointerEventButton, WindowEvent};
        if !is_child() {
            let Some(server) = crate::test_support::Xvfb::start() else {
                eprintln!("skipped: Xvfb is not installed");
                return;
            };
            return run_in_child(
                "ui::tests::the_about_dialog_grows_to_keep_its_button_in_the_window",
                &[("DISPLAY", OsStr::new(server.display()))],
                &["WAYLAND_DISPLAY", "FLATPAK_ID"],
            );
        }
        select_backend().unwrap();
        let dialogs: Vec<_> = [13.333_f32, 20.0, 26.0]
            .into_iter()
            .map(|font_size| {
                let mut tokens =
                    crate::theme::build(&crate::system_theme::preset(&Default::default(), false));
                tokens.font_size = font_size;
                let dialog = new_dialog(super::AboutDialog::new).unwrap();
                super::apply_theme(&dialog, &tokens);
                dialog.set_version("protonctx v1.5.3".into());
                let pressed = std::rc::Rc::new(std::cell::Cell::new(false));
                let p = pressed.clone();
                dialog.on_ok(move || p.set(true));
                dialog.show().unwrap();
                (dialog, pressed)
            })
            .collect();
        slint::Timer::single_shot(std::time::Duration::from_millis(500), || {
            let _ = slint::quit_event_loop();
        });
        slint::run_event_loop_until_quit().unwrap();

        let heights: Vec<u32> = dialogs
            .iter()
            .map(|(d, _)| d.window().size().height)
            .collect();
        assert!(
            heights[0] <= 201 && heights[1] > heights[0] && heights[2] > heights[1],
            "{heights:?}"
        );
        for (dialog, pressed) in &dialogs {
            let window = dialog.window();
            let size = window.size().to_logical(window.scale_factor());
            // Somewhere in the window's bottom-right quarter is the OK button.
            let found = (0..(size.height / 2.0) as i32).any(|dy| {
                (0..(size.width / 2.0) as i32).step_by(4).any(|dx| {
                    let position = slint::LogicalPosition::new(
                        size.width - 1.0 - dx as f32,
                        size.height - 1.0 - dy as f32,
                    );
                    let button = PointerEventButton::Left;
                    window.dispatch_event(WindowEvent::PointerMoved { position });
                    window.dispatch_event(WindowEvent::PointerPressed { position, button });
                    window.dispatch_event(WindowEvent::PointerReleased { position, button });
                    pressed.get()
                })
            });
            assert!(found, "OK in a {size:?} window");
        }
    }

    /// On the real winit backend under Xvfb, copying puts the text on the X11
    /// CLIPBOARD selection, where another client (a paste) reads it.
    #[test]
    fn copied_text_is_served_on_the_x11_clipboard() {
        use slint::ComponentHandle as _;
        use x11rb::connection::Connection as _;
        use x11rb::protocol::Event;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};
        if !is_child() {
            let Some(server) = crate::test_support::Xvfb::start() else {
                eprintln!("skipped: Xvfb is not installed");
                return;
            };
            return run_in_child(
                "ui::tests::copied_text_is_served_on_the_x11_clipboard",
                &[("DISPLAY", OsStr::new(server.display()))],
                &["WAYLAND_DISPLAY", "FLATPAK_ID"],
            );
        }
        select_backend().unwrap();
        let main = super::MainWindow::new().unwrap();
        main.show().unwrap();
        slint::Timer::single_shot(std::time::Duration::from_millis(300), || {
            let _ = slint::quit_event_loop();
        });
        slint::run_event_loop_until_quit().unwrap();
        let copied = "/home/u/.local/share/Steam/steamapps/compatdata/274190";
        main.invoke_copy_text(copied.into());

        // Paste, as another X client: ask the CLIPBOARD owner for UTF-8 text.
        let (conn, screen) = x11rb::connect(None).unwrap();
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            0,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap();
        let atom = |name: &str| {
            conn.intern_atom(false, name.as_bytes())
                .unwrap()
                .reply()
                .unwrap()
                .atom
        };
        let (clipboard, utf8, property) = (
            atom("CLIPBOARD"),
            atom("UTF8_STRING"),
            atom("PROTONCTX_PASTE"),
        );
        conn.convert_selection(window, clipboard, utf8, property, x11rb::CURRENT_TIME)
            .unwrap();
        conn.flush().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let notified = loop {
            match conn.poll_for_event().unwrap() {
                Some(Event::SelectionNotify(event)) => break event,
                Some(_) => {}
                None if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                None => panic!("no answer from the clipboard owner"),
            }
        };
        assert_eq!(notified.property, property, "the owner converted the text");
        let reply = conn
            .get_property(true, window, property, AtomEnum::ANY, 0, 1 << 16)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(String::from_utf8(reply.value).unwrap(), copied);
    }
}
