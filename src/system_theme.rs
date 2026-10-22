//! Static desktop theming at startup.
//!
//! KDE is the supported desktop: `native-theme` reads the live `kdeglobals`
//! scheme (colors, typography, DPI, accessibility) once at startup and the
//! active variant is handed to [`crate::theme`], which builds the app's
//! design tokens from it. Live scheme changes are out of
//! scope. Off KDE (and in tests, which must stay deterministic) the bundled
//! Breeze preset is used instead. Before use, the resolved variant also has
//! its fonts re-derived at the base logical DPI and its scrollbar model
//! re-derived from the palette, so every scrollbar paints the Qt/Breeze
//! track and handle.

use std::sync::mpsc;
use std::time::Duration;

use native_theme::color::Rgba;
use native_theme::theme::{ColorMode, FontSize, ResolvedFontSpec, ResolvedTheme};
use native_theme::{AccessibilityPreferences, SystemTheme};

/// Breeze preset used when the platform reader fails or the desktop is not
/// KDE.
pub(crate) const FALLBACK_PRESET: &str = "kde-breeze";

/// Base logical DPI for point-stated desktop font sizes. iced already applies
/// the display scale factor (the Wayland compositor scale; `Xft.dpi / 96` on
/// X11) to every logical pixel, so resolving points at the display DPI would
/// apply that same signal twice. Desktop point sizes therefore resolve at the
/// base logical DPI, as Qt does.
const BASE_DPI: f32 = 96.0;

/// Upper bound on the startup theme read. On KDE the reader makes a portal
/// D-Bus call that has no timeout of its own; a stalled portal must not block
/// startup before the first window opens.
const THEME_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Breeze scrollbar metrics (`kstyle/breezemetrics.h`): the groove is
/// `ScrollBar_Extend`, the handle `ScrollBar_SliderWidth` with
/// `ScrollBar_MinSliderHeight` as the minimum length.
///
/// The groove is pinned to the 16 px rail the gpui build used instead of
/// Breeze's 21 px, so the table and log keep the same content width; the
/// Breeze handle width is kept and centred with 4 px on both sides.
pub(crate) const SCROLLBAR_RAIL_WIDTH: f32 = 16.0;
pub(crate) const SCROLLBAR_THUMB_WIDTH: f32 = 8.0;
pub(crate) const SCROLLBAR_MIN_THUMB_LENGTH: f32 = 20.0;

/// Breeze paints the resting handle as `QPalette::WindowText` at 50 % over
/// the window fill (`scrollBarHandleColor` / `renderScrollBarHandle`); the
/// alpha is kept so it composites over the groove as Qt does.
const SCROLLBAR_THUMB_ALPHA: f32 = 0.5;

/// The theme the application runs with: the resolved variant, its display
/// name, and the accessibility preferences that scale its text.
#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub resolved: ResolvedTheme,
    pub is_dark: bool,
    pub prefs: AccessibilityPreferences,
}

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
fn normalize_resolved_fonts(resolved: &mut ResolvedTheme) {
    resolved.defaults.font.size = font_size_at_base_dpi(&resolved.defaults.font);
    resolved.defaults.mono_font.size = font_size_at_base_dpi(&resolved.defaults.mono_font);
}

/// `color` with its alpha scaled by `factor`, RGB untouched.
fn scale_alpha(color: Rgba, factor: f32) -> Rgba {
    Rgba::new(
        color.r,
        color.g,
        color.b,
        (f32::from(color.a) * factor).round() as u8,
    )
}

/// Re-derives the scrollbar model from the resolved KDE palette so every
/// scrollbar — games table, log pane, dialogs — paints the Qt/Breeze track
/// and handle in all states:
///
/// - track: the window fill, Breeze's groove colour;
/// - thumb: the window text at [`SCROLLBAR_THUMB_ALPHA`] over the groove,
///   Breeze's resting handle (light on Breeze Dark, dark on Breeze Light);
/// - hover and pressed/drag: the selection background, the accent the Qt
///   reference shows while the handle is hovered, kept while the handle is
///   dragged so the grab reads as active;
/// - geometry: 16 px rail / 8 px handle / 20 px minimum, centred.
fn normalize_resolved_scrollbar(resolved: &mut ResolvedTheme) {
    let thumb = scale_alpha(resolved.defaults.text_color, SCROLLBAR_THUMB_ALPHA);
    resolved.scrollbar.groove_width = SCROLLBAR_RAIL_WIDTH;
    resolved.scrollbar.thumb_width = SCROLLBAR_THUMB_WIDTH;
    resolved.scrollbar.min_thumb_length = SCROLLBAR_MIN_THUMB_LENGTH;
    resolved.scrollbar.track_color = resolved.defaults.background_color;
    resolved.scrollbar.thumb_color = thumb;
    resolved.scrollbar.thumb_hover_color = resolved.defaults.selection_background;
    resolved.scrollbar.thumb_active_color = Some(resolved.defaults.selection_background);
}

/// Applies both normalizations; every theme the app uses goes through this.
fn normalize(resolved: &mut ResolvedTheme) {
    normalize_resolved_fonts(resolved);
    normalize_resolved_scrollbar(resolved);
}

/// Reads the desktop theme once at startup.
///
/// `preset_mode` forces the bundled Breeze preset in that mode (the
/// `--preset light|dark` development switch). Otherwise the live KDE scheme
/// is read on KDE, and the Breeze preset in the system's mode is used off KDE
/// or when the reader fails.
pub fn load(preset_mode: Option<bool>) -> LoadedTheme {
    if let Some(is_dark) = preset_mode {
        return preset(&AccessibilityPreferences::default(), is_dark);
    }

    if !is_kde(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()) {
        return preset(
            &AccessibilityPreferences::default(),
            native_theme::detect::system_is_dark(),
        );
    }

    match read_system_theme() {
        Ok(system) => {
            let is_dark = system.mode == ColorMode::Dark;
            let mut resolved = system.pick(system.mode).clone();
            normalize(&mut resolved);
            LoadedTheme {
                resolved,
                is_dark,
                prefs: system.accessibility,
            }
        }
        Err(error) => {
            eprintln!(
                "protonctx: could not read the KDE theme ({error}); using the {FALLBACK_PRESET} preset"
            );
            // Defaults, not `from_system()`: the reader already failed or
            // timed out, so a second platform read would add another stall.
            // `system_is_dark` reads `kdeglobals` directly on KDE.
            preset(
                &AccessibilityPreferences::default(),
                native_theme::detect::system_is_dark(),
            )
        }
    }
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

/// The normalized Breeze preset for `is_dark`. The mode is chosen by the
/// caller so this stays deterministic; the preferences are a parameter so
/// tests can pass [`AccessibilityPreferences::default`] and stay I/O-free.
///
/// The bundled preset always resolves; should it ever fail, the failure is
/// reported and iced's built-in palette colours are used by the caller via
/// the same preset in the other mode.
pub fn preset(prefs: &AccessibilityPreferences, is_dark: bool) -> LoadedTheme {
    let resolve = |is_dark: bool| -> native_theme::Result<ResolvedTheme> {
        let spec = native_theme::theme::Theme::preset(FALLBACK_PRESET)?;
        let variant = spec.into_variant(if is_dark {
            ColorMode::Dark
        } else {
            ColorMode::Light
        })?;
        variant.resolve_system()
    };

    let (mut resolved, is_dark) = match resolve(is_dark) {
        Ok(resolved) => (resolved, is_dark),
        Err(error) => {
            eprintln!("protonctx: the {FALLBACK_PRESET} preset failed to resolve ({error})");
            match resolve(!is_dark) {
                Ok(resolved) => (resolved, !is_dark),
                // Both bundled variants failing means the binary itself is
                // broken; there is no theme data left to fall back to.
                Err(error) => panic!("the bundled {FALLBACK_PRESET} preset is unusable: {error}"),
            }
        }
    };
    normalize(&mut resolved);
    LoadedTheme {
        resolved,
        is_dark,
        prefs: prefs.clone(),
    }
}

/// Whether the KDE reader would be selected for `XDG_CURRENT_DESKTOP` (a
/// colon-separated list). Delegates to `native-theme`'s own detection so the
/// guard and the reader cannot disagree (exact case, first recognized wins).
#[cfg(target_os = "linux")]
fn is_kde(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|desktop| {
        native_theme::detect::parse_linux_desktop(desktop)
            == native_theme::detect::LinuxDesktop::Kde
    })
}

/// There is only a KDE reader on Linux.
#[cfg(not(target_os = "linux"))]
fn is_kde(_desktop: Option<&str>) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use native_theme::color::Rgba;
    use native_theme::theme::{FontSize, FontStyle, ResolvedFontSpec};

    use super::{
        AccessibilityPreferences, font_size_at_base_dpi, is_kde, normalize_resolved_fonts, preset,
    };

    #[test]
    #[cfg(target_os = "linux")]
    fn is_kde_matches_the_native_theme_detection() {
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
            color: Rgba::BLACK,
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
    /// re-derives them at the base logical DPI so the window scale factor is
    /// the only display-scale multiplier left.
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
        // The toolbar's Header source, distinct from the popup's Window
        // source.
        assert_eq!(resolved.list.header_background, Rgba::rgb(41, 44, 48));

        normalize_resolved_fonts(&mut resolved);

        assert_eq!(resolved.defaults.font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(resolved.defaults.mono_font.size, 10.0 * 96.0 / 72.0);
    }

    #[test]
    fn preset_selects_the_requested_variant_and_normalizes_it() {
        let light = preset(&AccessibilityPreferences::default(), false);
        assert!(!light.is_dark);
        let r = &light.resolved;
        assert_eq!(r.defaults.background_color, Rgba::rgb(0xEF, 0xF0, 0xF1));
        // 10 pt at the base logical DPI, independent of the display DPI.
        assert_eq!(r.defaults.font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(r.defaults.mono_font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(r.scrollbar.groove_width, 16.0);
        assert_eq!(r.scrollbar.thumb_width, 8.0);
        assert_eq!(r.scrollbar.min_thumb_length, 20.0);
        assert_eq!(r.scrollbar.track_color, Rgba::rgb(0xEF, 0xF0, 0xF1));
        assert_eq!(r.scrollbar.thumb_color, Rgba::new(0x23, 0x26, 0x29, 128));
        assert_eq!(r.scrollbar.thumb_hover_color, Rgba::rgb(0x3D, 0xAE, 0xE9));
        assert_eq!(
            r.scrollbar.thumb_active_color,
            Some(r.scrollbar.thumb_hover_color)
        );

        let dark = preset(&AccessibilityPreferences::default(), true);
        assert!(dark.is_dark);
        let r = &dark.resolved;
        assert_eq!(r.defaults.background_color, Rgba::rgb(0x20, 0x23, 0x26));
        assert_eq!(r.defaults.font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(r.defaults.mono_font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(r.scrollbar.track_color, Rgba::rgb(0x20, 0x23, 0x26));
        assert_eq!(r.scrollbar.thumb_color, Rgba::new(0xfc, 0xfc, 0xfc, 128));
        assert_eq!(r.scrollbar.thumb_hover_color, Rgba::rgb(0x3D, 0xAE, 0xE9));
        assert_eq!(
            r.scrollbar.thumb_active_color,
            Some(r.scrollbar.thumb_hover_color)
        );
    }
}
