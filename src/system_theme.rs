//! Static desktop theming at startup.
//!
//! KDE is the supported desktop: `native-theme` reads the live `kdeglobals`
//! scheme (colors, typography, DPI, accessibility) once at startup and the
//! active variant is handed to [`crate::theme`], which builds the app's
//! design tokens from it. Live scheme changes are out of
//! scope. The light/dark preference comes from the XDG Settings portal
//! (`org.freedesktop.appearance` `color-scheme`), which a Flatpak sandbox sees
//! too. Off KDE, when the scheme contradicts that preference, and in tests
//! (which must stay deterministic) the bundled Breeze preset is used instead,
//! in the preferred mode. Before use, the resolved variant has its
//! fonts re-derived at the base logical DPI. Its scrollbar model is not used:
//! [`crate::theme`] paints Breeze scroll bars from its own constants.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use native_theme::theme::{ColorMode, FontSize, ResolvedFontSpec, ResolvedTheme};
use native_theme::{AccessibilityPreferences, SystemTheme};

/// Breeze preset used when the platform reader fails or the desktop is not
/// KDE.
pub(crate) const FALLBACK_PRESET: &str = "kde-breeze";

/// Base logical DPI for point-stated desktop font sizes. Slint already applies
/// the display scale factor (the Wayland compositor scale; `Xft.dpi / 96` on
/// X11) to every logical pixel, so resolving points at the display DPI would
/// apply that same signal twice. Desktop point sizes therefore resolve at the
/// base logical DPI, as Qt does.
const BASE_DPI: f32 = 96.0;

/// Upper bound on the startup theme reads (the KDE reader and the Settings
/// portal, which run side by side). Both make portal D-Bus calls that have no
/// timeout of their own; a stalled portal must not block startup before the
/// first window opens.
const THEME_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The theme the application runs with: the resolved variant, its display
/// name, and the accessibility preferences that scale its text.
#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub resolved: ResolvedTheme,
    pub is_dark: bool,
    pub prefs: AccessibilityPreferences,
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

/// Re-derives the base UI and mono fonts of `resolved` at [`BASE_DPI`]; every
/// theme the app uses goes through this.
fn normalize_resolved_fonts(resolved: &mut ResolvedTheme) {
    resolved.defaults.font.size = font_size_at_base_dpi(&resolved.defaults.font);
    resolved.defaults.mono_font.size = font_size_at_base_dpi(&resolved.defaults.mono_font);
}

/// Reads the desktop theme once at startup.
///
/// `preset_mode` forces the bundled Breeze preset in that mode (the
/// `--preset light|dark` development switch). Otherwise the light/dark
/// preference comes from the XDG Settings portal, which answers the same
/// inside a Flatpak sandbox as outside it. On KDE the live scheme supplies the
/// colours, as long as it agrees with that preference: a scheme the reader
/// cannot see (a sandbox without access to `kdeglobals`, a desktop default
/// the user never overrode) reads as plain light Breeze. Off KDE, on a
/// disagreement, or when the reader fails, the Breeze preset is used in the
/// preferred mode.
pub fn load(preset_mode: Option<bool>) -> LoadedTheme {
    if let Some(is_dark) = preset_mode {
        return preset(&AccessibilityPreferences::default(), is_dark);
    }

    let deadline = Instant::now() + THEME_READ_TIMEOUT;
    let portal = spawn_read("color-scheme-read", portal_prefers_dark);
    let kde = is_kde(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()).then(|| {
        spawn_read("system-theme-read", || {
            SystemTheme::from_system().map_err(|e| e.to_string())
        })
    });
    let portal_dark = portal
        .and_then(|read| wait_read(read, deadline))
        .unwrap_or_else(|error| {
            eprintln!("protonctx: could not read the color-scheme preference ({error})");
            None
        });
    // The preference, or the platform's own guess without one.
    let preferred_dark = || portal_dark.unwrap_or_else(native_theme::detect::system_is_dark);

    let Some(kde) = kde else {
        return preset(&AccessibilityPreferences::default(), preferred_dark());
    };
    match kde.and_then(|read| wait_read(read, deadline)) {
        Ok(system) => {
            let is_dark = system.mode == ColorMode::Dark;
            if let Some(want_dark) = preset_mode_over_scheme(portal_dark, is_dark) {
                eprintln!(
                    "protonctx: the KDE colour scheme is {}, but the desktop prefers {}; using the {FALLBACK_PRESET} preset",
                    mode_name(is_dark),
                    mode_name(want_dark),
                );
                return preset(&system.accessibility, want_dark);
            }
            let mut resolved = system.pick(system.mode).clone();
            normalize_resolved_fonts(&mut resolved);
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
            preset(&AccessibilityPreferences::default(), preferred_dark())
        }
    }
}

/// `"dark"` or `"light"`.
fn mode_name(is_dark: bool) -> &'static str {
    if is_dark { "dark" } else { "light" }
}

/// The mode to use the Breeze preset in instead of the KDE scheme, when the
/// desktop states a preference (`portal_dark`) the scheme's own mode
/// contradicts; `None` keeps the scheme.
fn preset_mode_over_scheme(portal_dark: Option<bool>, scheme_is_dark: bool) -> Option<bool> {
    portal_dark.filter(|&want_dark| want_dark != scheme_is_dark)
}

/// The desktop's light/dark preference from the XDG Settings portal
/// (`org.freedesktop.appearance` `color-scheme`): `Some(true)` for dark,
/// `Some(false)` for light, `None` when the desktop states no preference.
fn portal_prefers_dark() -> Result<Option<bool>, String> {
    use ashpd::desktop::settings::Settings;
    async_io::block_on(async {
        let settings = Settings::new().await?;
        settings.color_scheme().await
    })
    .map(prefers_dark)
    .map_err(|error| error.to_string())
}

/// The portal's `color-scheme` value as a mode: 1 is dark, 2 is light, and 0
/// (no preference) is none.
fn prefers_dark(scheme: ashpd::desktop::settings::ColorScheme) -> Option<bool> {
    use ashpd::desktop::settings::ColorScheme;
    match scheme {
        ColorScheme::PreferDark => Some(true),
        ColorScheme::PreferLight => Some(false),
        ColorScheme::NoPreference => None,
    }
}

/// Start `read` on a worker thread named `name`; its outcome arrives on the
/// returned channel (see [`wait_read`]). Platform reads make D-Bus calls with
/// no timeout of their own, so they never run on the startup thread itself.
fn spawn_read<T: Send + 'static>(
    name: &str,
    read: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<mpsc::Receiver<Result<T, String>>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let _ = sender.send(read());
        })
        .map(|_| receiver)
        .map_err(|error| format!("could not start the {name} thread ({error})"))
}

/// The outcome of a [`spawn_read`] by `deadline`, returning the failure
/// reason instead of hanging. A timed-out reader is left to finish and drop
/// its result.
fn wait_read<T>(
    receiver: mpsc::Receiver<Result<T, String>>,
    deadline: Instant,
) -> Result<T, String> {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "the platform read did not finish within {THEME_READ_TIMEOUT:?}"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the platform reader stopped unexpectedly".to_string())
        }
    }
}

/// The normalized Breeze preset for `is_dark`. The mode is chosen by the
/// caller so this stays deterministic; the preferences are a parameter so
/// tests can pass [`AccessibilityPreferences::default`] and stay I/O-free.
///
/// The bundled preset always resolves; should it ever fail, the failure is
/// reported and the same preset is resolved in the other mode.
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
    normalize_resolved_fonts(&mut resolved);
    LoadedTheme {
        resolved,
        is_dark,
        prefs: prefs.clone(),
    }
}

/// Whether the KDE reader would be selected for `XDG_CURRENT_DESKTOP` (a
/// colon-separated list). Delegates to `native-theme`'s own detection so the
/// guard and the reader cannot disagree (exact case, first recognized wins).
fn is_kde(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|desktop| {
        native_theme::detect::parse_linux_desktop(desktop)
            == native_theme::detect::LinuxDesktop::Kde
    })
}

#[cfg(test)]
mod tests {
    use native_theme::color::Rgba;
    use native_theme::theme::{FontSize, FontStyle, ResolvedFontSpec};

    use super::{
        AccessibilityPreferences, font_size_at_base_dpi, is_kde, normalize_resolved_fonts,
        prefers_dark, preset, preset_mode_over_scheme, spawn_read, wait_read,
    };

    #[test]
    fn the_portal_color_scheme_maps_to_a_mode() {
        use ashpd::desktop::settings::ColorScheme;
        assert_eq!(prefers_dark(ColorScheme::PreferDark), Some(true));
        assert_eq!(prefers_dark(ColorScheme::PreferLight), Some(false));
        assert_eq!(prefers_dark(ColorScheme::NoPreference), None);
    }

    #[test]
    fn a_scheme_against_the_desktop_preference_gives_way_to_the_preset() {
        // A sandbox without kdeglobals reads as light Breeze on a dark desktop.
        assert_eq!(preset_mode_over_scheme(Some(true), false), Some(true));
        assert_eq!(preset_mode_over_scheme(Some(false), true), Some(false));
        // An agreeing scheme, or no stated preference, keeps the KDE colours.
        assert_eq!(preset_mode_over_scheme(Some(true), true), None);
        assert_eq!(preset_mode_over_scheme(Some(false), false), None);
        assert_eq!(preset_mode_over_scheme(None, true), None);
        assert_eq!(preset_mode_over_scheme(None, false), None);
    }

    #[test]
    fn a_stalled_read_gives_up_at_the_deadline() {
        let read = spawn_read("test-stalled-read", || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(())
        })
        .unwrap();
        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_millis(50);
        assert!(wait_read(read, deadline).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));

        let read = spawn_read("test-quick-read", || Ok(7)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        assert_eq!(wait_read(read, deadline), Ok(7));
    }

    #[test]
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

        let dark = preset(&AccessibilityPreferences::default(), true);
        assert!(dark.is_dark);
        let r = &dark.resolved;
        assert_eq!(r.defaults.background_color, Rgba::rgb(0x20, 0x23, 0x26));
        assert_eq!(r.defaults.font.size, 10.0 * 96.0 / 72.0);
        assert_eq!(r.defaults.mono_font.size, 10.0 * 96.0 / 72.0);
    }
}
