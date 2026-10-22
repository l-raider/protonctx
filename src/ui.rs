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

/// Whether an X server (XWayland on a Plasma Wayland session) is available.
fn has_x11_display() -> bool {
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

    g.set_scrollbar_track(c(t.scrollbar_track));
    g.set_scrollbar_thumb(c(t.scrollbar_thumb));
    g.set_scrollbar_thumb_hover(c(t.scrollbar_thumb_hover));
    g.set_scrollbar_thumb_active(c(t.scrollbar_thumb_active));
    g.set_scrollbar_width(t.scrollbar_width);
    g.set_scroller_width(t.scroller_width);
    g.set_scrollbar_min_thumb(t.scrollbar_min_thumb);

    g.set_font_family(t.font_family.as_str().into());
    g.set_font_weight(i32::from(t.font_weight));
    g.set_font_size(t.font_size);
    g.set_mono_family(t.mono_family.as_str().into());
    g.set_mono_size(t.mono_size);
    g.set_reduce_motion(t.reduce_motion);
}
