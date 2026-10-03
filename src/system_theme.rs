//! Static desktop theming at startup.
//!
//! KDE is the supported desktop: `native-theme-gpui` reads the live
//! `kdeglobals` scheme (colors, typography, DPI, accessibility) and installs
//! both the light and dark variants, so gpui-component's `Theme::change`
//! switches between native palettes with no custom detection. The theme is
//! read once at startup; live scheme changes are out of scope. Off KDE, and
//! under GPUI's test scheduler, this is a no-op.

use std::sync::mpsc;
use std::time::Duration;

use gpui_kit::component::{Theme, scroll::ScrollbarMode};
use gpui_kit::*;
use native_theme::theme::{FontSize, ResolvedFontSpec, ResolvedTheme};
use native_theme_gpui::{AccessibilityPreferences, SystemTheme, apply_system_theme, from_preset};

/// Breeze preset installed when the platform reader fails.
const FALLBACK_PRESET: &str = "kde-breeze";

/// Alpha of the KDE selection color used for hover accents.
const ACCENT_ALPHA: f32 = 0.3;

/// Base logical DPI for point-stated desktop font sizes. GPUI already applies
/// the display scale factor (the Wayland compositor scale; `Xft.dpi / 96` on
/// X11) to every logical pixel, so resolving points at the display DPI bakes
/// that same signal into `Theme.font_size` and scales the rem axis twice.
/// Desktop point sizes therefore resolve at the base logical DPI, as Qt does.
const BASE_DPI: f32 = 96.0;

/// Upper bound on the startup theme read. On KDE the reader makes a portal
/// D-Bus call that has no timeout of its own; a stalled portal must not block
/// the UI thread before the first window opens.
const THEME_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Result of the worker-thread theme read; the error is stringified so it can
/// cross the thread boundary.
enum ThemeRead {
    Ready(Box<SystemTheme>),
    Failed(String),
}

/// The logical-pixel size `font` should render at: a point-stated size is
/// re-derived at [`BASE_DPI`], while a pixel-stated or unstated size passes
/// through unchanged.
fn font_size_at_base_dpi(font: &ResolvedFontSpec) -> f32 {
    match font.defined_size {
        Some(FontSize::Pt(pt)) => pt * BASE_DPI / 72.0,
        _ => font.size,
    }
}

/// Re-derives the base UI and mono fonts of `resolved` at [`BASE_DPI`].
///
/// Both variants must be normalized before `apply_system_theme` / `to_theme`:
/// the flat `Theme.font_size`, the stored variant, and both `ThemeConfig`s
/// would otherwise disagree with each other and with the intended scale.
fn normalize_resolved_fonts(resolved: &mut ResolvedTheme) {
    resolved.defaults.font.size = font_size_at_base_dpi(&resolved.defaults.font);
    resolved.defaults.mono_font.size = font_size_at_base_dpi(&resolved.defaults.mono_font);
}

/// Applies the desktop theme once at startup. No-op off KDE and under GPUI's
/// test scheduler, which must stay deterministic.
pub(crate) fn init(cx: &mut App) {
    if !is_kde(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()) || is_test_scheduler(cx) {
        return;
    }

    match read_system_theme() {
        Ok(mut system) => {
            normalize_resolved_fonts(&mut system.light);
            normalize_resolved_fonts(&mut system.dark);
            apply_system_theme(&system, cx);
        }
        Err(error) => {
            eprintln!(
                "protonctx: could not read the KDE theme ({error}); using the {FALLBACK_PRESET} preset"
            );
            // Defaults, not `from_system()`: the reader already failed or
            // timed out, so a second platform read would add another stall.
            // The mode comes from the crate's own file-based detection, not
            // from `cx.window_appearance()`: on Linux the platform appearance
            // starts as Light and is only corrected once the portal answers
            // (which also happens after the first frame), so reading it here
            // would flash the wrong variant. `system_is_dark` reads
            // `kdeglobals` directly on KDE.
            install_preset(
                &AccessibilityPreferences::default(),
                native_theme::detect::system_is_dark(),
                cx,
            );
        }
    }

    // Breeze keeps scrollbars visible; the connector derives its mode from the
    // scheme and would hide them in overlay mode.
    Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);
    restore_breeze_traits(cx);
}

/// Reads the system theme on a worker thread and waits at most
/// [`THEME_READ_TIMEOUT`], returning the failure reason instead of hanging.
/// A timed-out reader is left to finish and drop its result.
fn read_system_theme() -> Result<SystemTheme, String> {
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::Builder::new()
        .name("system-theme-read".to_string())
        .spawn(move || {
            let outcome = match SystemTheme::from_system() {
                Ok(system) => ThemeRead::Ready(Box::new(system)),
                Err(error) => ThemeRead::Failed(error.to_string()),
            };
            let _ = sender.send(outcome);
        });
    if let Err(error) = reader {
        return Err(format!("could not start the theme reader ({error})"));
    }

    match receiver.recv_timeout(THEME_READ_TIMEOUT) {
        Ok(ThemeRead::Ready(system)) => Ok(*system),
        Ok(ThemeRead::Failed(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "the platform read did not finish within {THEME_READ_TIMEOUT:?}"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the theme reader stopped unexpectedly".to_string())
        }
    }
}

/// Installs the Breeze preset for `is_dark`; used when the platform reader
/// fails. The mode is chosen by the caller so this stays deterministic and
/// never consults the platform appearance, which is stale before the portal
/// answers. The preferences are a parameter so tests can pass a deterministic
/// [`AccessibilityPreferences::default`] and stay I/O-free.
pub(crate) fn install_preset(prefs: &AccessibilityPreferences, is_dark: bool, cx: &mut App) {
    match from_preset(FALLBACK_PRESET, is_dark, prefs) {
        Ok((theme, mut resolved)) => {
            // `from_preset` already built the theme from the display-DPI
            // resolution; rebuild it from the normalized variant so the flat
            // theme, its `ThemeConfig`, and the stored variant agree.
            normalize_resolved_fonts(&mut resolved);
            let name = theme.theme_name().clone();
            let theme = native_theme_gpui::to_theme(&resolved, &name, is_dark, prefs);
            native_theme_gpui::apply(theme, &resolved, prefs, cx);
        }
        Err(error) => {
            eprintln!("protonctx: the {FALLBACK_PRESET} preset failed to resolve ({error})")
        }
    }
}

/// Re-asserts Breeze's focus, panel, accent and selection treatment over a
/// reloaded config:
///
/// - Breeze draws focus as a 1px border, not an outer ring.
/// - The app paints panels (the log pane) with `muted`, which Breeze fills
///   with the view background; the connector derives `muted` from the window
///   fill instead.
/// - The app tints hovered menu items with the live selection color at
///   [`ACCENT_ALPHA`]; the connector uses Breeze's stated menu hover color.
/// - Hovered table rows take that same selection tint, so table and menu
///   hovers share one source instead of Breeze's separate list hover color.
/// - KDE selections are solid where gpui-component clamps them to a wash.
/// - KDE draws menus (and popovers) on the window background; the connector
///   fills `popover` with the view background, a step darker in Breeze Dark.
///
/// No-op unless the connector installed a native theme.
pub(crate) fn restore_breeze_traits(cx: &mut App) {
    let Some((view_background, window_background, accent)) = cx
        .try_global::<native_theme_gpui::NativeTheme>()
        .and_then(|native| native.resolved(cx))
        .map(|resolved| {
            (
                rgba_to_hsla(resolved.defaults.surface_color),
                rgba_to_hsla(resolved.defaults.background_color),
                rgba_to_hsla(resolved.defaults.selection_background).opacity(ACCENT_ALPHA),
            )
        })
    else {
        return;
    };

    // Set the token too, or reconcile restores the clamped alpha.
    Theme::update(cx, |theme| {
        theme.focus_ring = false;
        theme.muted = view_background;
        theme.popover = window_background;
        theme.accent = accent;
        theme.selection = theme.selection.alpha(1.);
        theme.tokens.selection = theme.selection.into();
        theme.tokens.table_hover = accent.into();
    });
}

/// The connector's private `rgba_to_hsla` equivalent, for the model colours
/// the app re-reads itself.
fn rgba_to_hsla(color: native_theme_gpui::Rgba) -> Hsla {
    gpui_kit::Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
    .into()
}

/// Whether the application runs on GPUI's deterministic test scheduler, which
/// must not be woken by a portal answering from its own thread.
fn is_test_scheduler(cx: &App) -> bool {
    cx.background_executor()
        .scheduler_executor()
        .scheduler()
        .as_test()
        .is_some()
}

/// Whether the connector would select its KDE reader for `XDG_CURRENT_DESKTOP`
/// (a colon-separated list). Delegates to the connector's own detection so the
/// guard and the reader cannot disagree (exact case, first recognized wins).
#[cfg(target_os = "linux")]
fn is_kde(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|desktop| {
        native_theme::detect::parse_linux_desktop(desktop) == native_theme_gpui::LinuxDesktop::Kde
    })
}

/// The connector only has a KDE reader on Linux.
#[cfg(not(target_os = "linux"))]
fn is_kde(_desktop: Option<&str>) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::{Theme, ThemeMode, try_parse_color};
    use gpui_kit::{TestAppContext, px};
    use native_theme::theme::{FontSize, FontStyle, ResolvedFontSpec};

    use super::{
        AccessibilityPreferences, font_size_at_base_dpi, install_preset, is_kde, is_test_scheduler,
        normalize_resolved_fonts, restore_breeze_traits,
    };

    #[test]
    #[cfg(target_os = "linux")]
    fn is_kde_matches_the_connector_detection() {
        assert!(is_kde(Some("KDE")));
        assert!(is_kde(Some("KDE:GNOME")));
        assert!(!is_kde(Some("GNOME:KDE")));
        assert!(!is_kde(Some("kde")));
        assert!(!is_kde(Some("")));
        assert!(!is_kde(None));
    }

    #[test]
    fn font_size_at_base_dpi_re_derives_points_and_keeps_pixels() {
        let spec = |size: f32, defined_size: Option<FontSize>| ResolvedFontSpec {
            family: "Noto Sans".into(),
            size,
            defined_size,
            weight: 400,
            style: FontStyle::Normal,
            color: native_theme_gpui::Rgba::BLACK,
        };

        // A 10 pt size resolved at a 192 display DPI is re-derived at the
        // base logical DPI.
        let points = spec(10.0 * 192.0 / 72.0, Some(FontSize::Pt(10.0)));
        assert_eq!(font_size_at_base_dpi(&points), 10.0 * 96.0 / 72.0);

        // Pixel-stated and unstated sizes pass through unchanged.
        assert_eq!(
            font_size_at_base_dpi(&spec(14.0, Some(FontSize::Px(14.0)))),
            14.0
        );
        assert_eq!(font_size_at_base_dpi(&spec(13.0, None)), 13.0);
    }

    /// The pinned contract: at a display DPI of 192, `native-theme` resolves
    /// KDE's 10 pt body and mono fonts to 26.67 logical px; normalization
    /// re-derives them at the base logical DPI so GPUI's window scale factor
    /// is the only display-scale multiplier left.
    #[test]
    #[cfg(target_os = "linux")]
    fn kde_point_fonts_resolved_at_display_dpi_are_normalized_to_base_dpi() {
        const KDE_INI: &str = "[General]\n\
            ColorScheme=BreezeLight\n\
            font=Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\
            fixed=Hack,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\
            [Colors:Header]\n\
            BackgroundNormal=41,44,48\n";

        let (reader, _, _) = native_theme::kde::from_kde_content_pure(KDE_INI, Some(192.0))
            .expect("the pure KDE reader must parse the test INI");
        // The real pipeline merges the sparse reader variant over the
        // platform preset before resolving; do the same so validation sees
        // the preset's colors.
        let mut variant = native_theme::theme::Theme::preset("kde-breeze")
            .expect("the Breeze preset is bundled")
            .into_variant(native_theme::theme::ColorMode::Light)
            .expect("the Breeze preset has a light variant");
        variant.merge(&reader.light.expect("the test INI states a light scheme"));

        let mut resolved = variant
            .into_resolved(&native_theme::resolve::ResolutionContext {
                font_dpi: 192.0,
                ..native_theme::resolve::ResolutionContext::for_tests()
            })
            .expect("the merged KDE variant must validate");

        assert_eq!(resolved.defaults.font.size, 10.0 * 192.0 / 72.0);
        assert_eq!(resolved.defaults.mono_font.size, 10.0 * 192.0 / 72.0);
        // The toolbar's Header source (`theme.list_head`), distinct from the
        // popup's Window source.
        assert_eq!(
            resolved.list.header_background,
            native_theme_gpui::Rgba::rgb(41, 44, 48)
        );

        normalize_resolved_fonts(&mut resolved);

        assert_eq!(resolved.defaults.font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(resolved.defaults.mono_font.size, 10.0 * 96.0 / 72.0);
    }

    #[gpui_kit::test]
    fn test_scheduler_is_detected(cx: &mut TestAppContext) {
        cx.update(|cx| assert!(is_test_scheduler(cx)));
    }

    #[gpui_kit::test]
    fn init_is_a_no_op_under_the_test_scheduler(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        let sentinel = try_parse_color("#123456").unwrap();
        cx.update(|cx| {
            Theme::update(cx, |theme| theme.background = sentinel);
            crate::system_theme::init(cx);

            assert_eq!(Theme::global(cx).background, sentinel);
            assert!(cx.try_global::<native_theme_gpui::NativeTheme>().is_none());
        });
    }

    #[gpui_kit::test]
    fn install_preset_selects_the_requested_variant(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);

        cx.update(|cx| install_preset(&AccessibilityPreferences::default(), false, cx));
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert_eq!(theme.mode, ThemeMode::Light);
            assert_eq!(theme.background, try_parse_color("#EFF0F1").unwrap());
            assert_eq!(theme.theme_name().as_ref(), "KDE Breeze");
            // 10 pt at the base logical DPI, independent of the display DPI.
            assert_eq!(theme.font_size, px(10.0 * 96.0 / 72.0));
            assert_eq!(theme.mono_font_size, px(10.0 * 96.0 / 72.0));
        });

        cx.update(|cx| install_preset(&AccessibilityPreferences::default(), true, cx));
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert_eq!(theme.mode, ThemeMode::Dark);
            assert_eq!(theme.background, try_parse_color("#202326").unwrap());
            assert_eq!(theme.font_size, px(10.0 * 96.0 / 72.0));
            assert_eq!(theme.mono_font_size, px(10.0 * 96.0 / 72.0));
        });

        cx.update(restore_breeze_traits);
        cx.update(|cx| {
            let theme = Theme::global(cx);
            assert!(!theme.focus_ring);
            assert_eq!(theme.muted, try_parse_color("#141618").unwrap());
            // KDE menus draw the window background, not the connector's
            // view-background popover fill.
            assert_eq!(theme.popover, try_parse_color("#202326").unwrap());
            let selection = try_parse_color("#3DAEE9").unwrap();
            assert_eq!(theme.accent, selection.opacity(0.3));
            assert_eq!(theme.selection.a, 1.0);
            assert_eq!(theme.tokens.selection.color, theme.selection);
            assert_eq!(theme.tokens.table_hover.color, theme.accent);
        });
    }
}
