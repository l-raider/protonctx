//! The iced palette and the app's token sidecar.
//!
//! [`build`] turns the resolved desktop theme into an `iced::Theme` (through
//! `native_theme_iced::to_theme`) plus an [`AppTokens`] value that carries
//! every colour and metric the views paint with, Breeze overrides included.
//! The tokens are installed once at startup ([`install`]) and read from style
//! closures through [`tokens`]; the theme never changes at runtime.

use std::sync::OnceLock;

use iced::font::{Family, Weight};
use iced::{Color, Font};
use native_theme::theme::{ResolvedFontSpec, ResolvedTheme};
use native_theme_iced::palette::to_color;

use crate::system_theme::LoadedTheme;

/// Alpha of the KDE selection colour used for hover accents (menu items and
/// table rows).
pub const ACCENT_ALPHA: f32 = 0.3;

/// KDE's frame contrast: `[KDE] contrast=4` → Kirigami `frameContrast` 0.2.
/// QtWidgets' frame colour (`QPalette::Mid`) is the window text blended over
/// the window fill at this weight, in sRGB.
const FRAME_CONTRAST: f32 = 0.2;

/// Weight of the text in a table grid line: `QTableView`'s grid is the view
/// fill moved 4 % towards the text.
const GRID_CONTRAST: f32 = 0.04;

/// Weight of the table header labels: Breeze draws headers at Medium, not
/// Bold.
pub const HEADER_WEIGHT: Weight = Weight::Medium;

/// Every colour and metric the views read, resolved once from the desktop
/// theme.
#[derive(Clone, Debug)]
pub struct AppTokens {
    pub is_dark: bool,

    // ---- Surfaces ----
    /// Window background (`QPalette::Window`).
    pub background: Color,
    /// Window text.
    pub foreground: Color,
    /// View background (`QPalette::Base`): the log pane and list fill.
    /// gpui-component's `muted`, which Breeze paints with the view colour.
    pub muted: Color,
    /// KDE Header colour set: the toolbar and the table header.
    pub header: Color,
    /// Header text.
    pub header_foreground: Color,
    /// List view fill (even rows).
    pub list_background: Color,
    /// Alternating row fill (odd rows), also the row separator shade.
    pub alternate_row: Color,
    /// List text.
    pub list_foreground: Color,
    /// Frame colour (`QPalette::Mid`): the table and log pane outlines.
    pub border: Color,
    /// Grid line: the table header's column separators.
    pub grid: Color,
    /// Secondary text (the About licence line).
    pub muted_foreground: Color,

    // ---- Controls ----
    /// Outline button border.
    pub input_border: Color,
    /// Outline button fill.
    pub input_background: Color,
    /// Button label colour.
    pub button_foreground: Color,
    /// Selection / default-action fill (`QPalette::Highlight`).
    pub primary: Color,
    /// Text on [`Self::primary`].
    pub primary_foreground: Color,
    /// Focus border.
    pub ring: Color,
    /// Hover tint: the selection at [`ACCENT_ALPHA`].
    pub accent: Color,
    /// The selection, forced opaque.
    pub selection: Color,
    /// Hover fill of the selected row: `highlight.lighter(110)`.
    pub selection_hover: Color,
    pub danger: Color,
    pub warning: Color,
    /// Corner radius of frames and buttons.
    pub radius: f32,
    /// Opacity of disabled controls.
    pub disabled_opacity: f32,

    // ---- Menus ----
    /// Popup menu fill: KDE draws menus on the window background.
    pub menu_background: Color,
    pub menu_foreground: Color,
    pub menu_border: Color,
    pub menu_border_width: f32,
    pub menu_radius: f32,

    // ---- Scrollbars ----
    pub scrollbar_track: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_thumb_hover: Color,
    pub scrollbar_thumb_active: Color,
    pub scrollbar_width: f32,
    pub scroller_width: f32,
    pub scrollbar_min_thumb: f32,

    // ---- Fonts ----
    pub font: Font,
    pub font_size: f32,
    pub header_font: Font,
    pub semibold_font: Font,
    pub mono_font: Font,
    pub mono_size: f32,
}

static TOKENS: OnceLock<AppTokens> = OnceLock::new();

/// Installs the startup tokens. Only the first call wins; the theme is static.
pub fn install(tokens: AppTokens) {
    let _ = TOKENS.set(tokens);
}

/// The installed tokens. Before [`install`] (unit tests) this is the
/// deterministic Breeze Dark preset.
pub fn tokens() -> &'static AppTokens {
    TOKENS.get_or_init(|| {
        let loaded = crate::system_theme::preset(&Default::default(), true);
        build(&loaded).1
    })
}

/// The iced theme and the token sidecar for `loaded`.
pub fn build(loaded: &LoadedTheme) -> (iced::Theme, AppTokens) {
    let theme = native_theme_iced::to_theme(&loaded.resolved, &loaded.name);
    (theme, tokens_for(loaded))
}

/// `color` with its alpha replaced by `alpha`.
fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// `foreground` blended over `background` at `weight`, in sRGB (not HSV,
/// which drifts the hue).
fn blend(foreground: Color, background: Color, weight: f32) -> Color {
    let mix = |f: f32, b: f32| f * weight + b * (1.0 - weight);
    Color {
        r: mix(foreground.r, background.r),
        g: mix(foreground.g, background.g),
        b: mix(foreground.b, background.b),
        a: 1.0,
    }
}

/// Qt's `QColor::lighter(factor)`: the HSV value is scaled by
/// `factor / 100`; any overflow above full value is taken out of the
/// saturation, so very bright colours wash towards white instead of clipping.
pub fn qt_lighter(color: Color, factor: f32) -> Color {
    let (r, g, b) = (color.r, color.g, color.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let hue = if delta <= f32::EPSILON {
        0.0
    } else if (max - r).abs() <= f32::EPSILON {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let mut saturation = if max <= f32::EPSILON { 0.0 } else { delta / max };
    let mut value = max * factor / 100.0;
    if value > 1.0 {
        saturation = (saturation - (value - 1.0)).max(0.0);
        value = 1.0;
    }

    let chroma = value * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = value - chroma;
    let (r, g, b) = match hue {
        h if h < 60.0 => (chroma, x, 0.0),
        h if h < 120.0 => (x, chroma, 0.0),
        h if h < 180.0 => (0.0, chroma, x),
        h if h < 240.0 => (0.0, x, chroma),
        h if h < 300.0 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    Color {
        r: r + m,
        g: g + m,
        b: b + m,
        a: color.a,
    }
}

/// A family name iced can address. iced's `Family::Name` takes a
/// `&'static str`; the theme is read once, so each family is leaked once.
fn family_of(spec: &ResolvedFontSpec) -> Family {
    let name = native_theme_iced::system_font_family(spec);
    Family::Name(Box::leak(name.to_string().into_boxed_str()))
}

/// The iced font for `spec`, at its stated weight.
fn font_of(spec: &ResolvedFontSpec) -> Font {
    Font {
        family: family_of(spec),
        weight: native_theme_iced::to_iced_weight(spec.weight),
        ..Font::DEFAULT
    }
}

fn tokens_for(loaded: &LoadedTheme) -> AppTokens {
    let r: &ResolvedTheme = &loaded.resolved;
    let d = &r.defaults;

    let background = to_color(d.background_color);
    let foreground = to_color(d.text_color);
    let view = to_color(d.surface_color);
    let selection = with_alpha(to_color(d.selection_background), 1.0);
    let input_border = to_color(r.input.border.color);
    // gpui-component's `input_background()`: the border mixed with
    // transparent at 0.3 in a dark theme, the window fill in a light one.
    let input_background = if loaded.is_dark {
        with_alpha(input_border, input_border.a * 0.3)
    } else {
        background
    };
    let primary = to_color(r.button.primary_background);

    let font = font_of(&d.font);
    let mono_font = font_of(&d.mono_font);

    AppTokens {
        is_dark: loaded.is_dark,

        background,
        foreground,
        muted: view,
        header: to_color(r.list.header_background),
        header_foreground: to_color(r.list.header_font.color),
        list_background: to_color(r.list.background_color),
        alternate_row: to_color(r.list.alternate_row_background),
        list_foreground: to_color(r.list.item_font.color),
        border: blend(foreground, background, FRAME_CONTRAST),
        grid: blend(foreground, view, GRID_CONTRAST),
        muted_foreground: to_color(d.muted_color),

        input_border,
        input_background,
        button_foreground: to_color(r.button.font.color),
        primary,
        primary_foreground: to_color(r.button.primary_text_color),
        ring: to_color(d.focus_ring_color),
        accent: with_alpha(selection, ACCENT_ALPHA),
        selection,
        selection_hover: qt_lighter(primary, 110.0),
        danger: to_color(d.danger_color),
        warning: to_color(d.warning_color),
        radius: d.border.corner_radius.max(0.0),
        disabled_opacity: 0.5,

        menu_background: background,
        menu_foreground: to_color(r.menu.font.color),
        menu_border: to_color(r.menu.border.color),
        menu_border_width: r.menu.border.line_width.max(0.0),
        menu_radius: r.menu.border.corner_radius.max(0.0),

        scrollbar_track: to_color(r.scrollbar.track_color),
        scrollbar_thumb: to_color(r.scrollbar.thumb_color),
        scrollbar_thumb_hover: to_color(r.scrollbar.thumb_hover_color),
        scrollbar_thumb_active: to_color(
            r.scrollbar
                .thumb_active_color
                .unwrap_or(r.scrollbar.thumb_hover_color),
        ),
        scrollbar_width: r.scrollbar.groove_width,
        scroller_width: r.scrollbar.thumb_width,
        scrollbar_min_thumb: r.scrollbar.min_thumb_length,

        font,
        font_size: native_theme_iced::font_size(r, &loaded.prefs),
        header_font: Font {
            weight: HEADER_WEIGHT,
            ..font
        },
        semibold_font: Font {
            weight: Weight::Semibold,
            ..font
        },
        mono_font,
        mono_size: native_theme_iced::mono_font_size(r, &loaded.prefs),
    }
}

#[cfg(test)]
mod tests {
    use iced::Color;

    use super::{ACCENT_ALPHA, build, qt_lighter};
    use crate::system_theme::preset;

    fn hex(rgb: u32) -> Color {
        Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    fn assert_close(actual: Color, expected: Color, what: &str) {
        let close = |a: f32, b: f32| (a - b).abs() < 1.0 / 255.0 + 1e-4;
        assert!(
            close(actual.r, expected.r)
                && close(actual.g, expected.g)
                && close(actual.b, expected.b)
                && close(actual.a, expected.a),
            "{what}: {actual:?} != {expected:?}"
        );
    }

    #[test]
    fn breeze_dark_tokens_match_the_pinned_values() {
        let loaded = preset(&Default::default(), true);
        let (theme, t) = build(&loaded);

        assert!(t.is_dark);
        assert_close(t.background, hex(0x202326), "window background");
        assert_close(theme.palette().background, hex(0x202326), "iced palette");
        assert_close(t.muted, hex(0x141618), "view / muted");
        // The preset's header is the view colour; the live KDE Header colour
        // (#292C30) is asserted through the KDE reader in `system_theme`.
        assert_close(t.header, hex(0x141618), "header");
        assert_close(t.alternate_row, hex(0x2A2E32), "alternate row");
        assert_close(t.foreground, hex(0xFCFCFC), "text");
        assert_close(t.selection, hex(0x3DAEE9), "selection");
        assert_close(
            t.accent,
            Color {
                a: ACCENT_ALPHA,
                ..hex(0x3DAEE9)
            },
            "accent",
        );
        assert_eq!(t.selection.a, 1.0);
        assert_close(t.border, hex(0x4C4E51), "frame");
        assert_close(
            t.scrollbar_thumb,
            Color::from_rgba8(252, 252, 252, 128.0 / 255.0),
            "scrollbar thumb",
        );
        assert_close(t.scrollbar_thumb_hover, hex(0x3DAEE9), "thumb hover");
        assert_eq!(t.scrollbar_thumb_active, t.scrollbar_thumb_hover);
        assert_close(t.scrollbar_track, hex(0x202326), "scrollbar track");
        assert_close(t.menu_background, t.background, "menu background");
        assert_eq!(t.scrollbar_width, 16.0);
        assert_eq!(t.scroller_width, 8.0);
        assert_eq!(t.scrollbar_min_thumb, 20.0);
        assert_eq!(t.font_size, 10.0 * 96.0 / 72.0);
        assert_eq!(t.mono_size, 10.0 * 96.0 / 72.0);
        assert_eq!(t.header_font.weight, iced::font::Weight::Medium);
    }

    #[test]
    fn breeze_dark_grid_is_close_to_the_pinned_shade() {
        let t = build(&preset(&Default::default(), true)).1;
        // ≈ #1C1E21: within a couple of steps per channel.
        let expected = hex(0x1C1E21);
        let near = |a: f32, b: f32| (a - b).abs() <= 3.0 / 255.0;
        assert!(
            near(t.grid.r, expected.r) && near(t.grid.g, expected.g) && near(t.grid.b, expected.b),
            "grid {:?}",
            t.grid
        );
    }

    #[test]
    fn breeze_light_tokens_match_the_pinned_values() {
        let t = build(&preset(&Default::default(), false)).1;

        assert!(!t.is_dark);
        assert_close(t.background, hex(0xEFF0F1), "window background");
        assert_close(t.scrollbar_track, hex(0xEFF0F1), "scrollbar track");
        assert_close(
            t.scrollbar_thumb,
            Color::from_rgba8(0x23, 0x26, 0x29, 128.0 / 255.0),
            "scrollbar thumb",
        );
        assert_close(t.scrollbar_thumb_hover, hex(0x3DAEE9), "thumb hover");
        // Light themes fill outline buttons with the window colour.
        assert_eq!(t.input_background, t.background);
    }

    #[test]
    fn qt_lighter_scales_value_and_washes_out_overflow() {
        // Pure value scaling below the ceiling.
        assert_close(
            qt_lighter(Color::from_rgb(0.5, 0.25, 0.0), 110.0),
            Color::from_rgb(0.55, 0.275, 0.0),
            "scaled",
        );
        // White stays white; grey just brightens.
        assert_close(qt_lighter(Color::WHITE, 110.0), Color::WHITE, "white");
        assert_close(
            qt_lighter(Color::from_rgb(0.5, 0.5, 0.5), 110.0),
            Color::from_rgb(0.55, 0.55, 0.55),
            "grey",
        );
        // A saturated colour at full value loses saturation instead.
        let washed = qt_lighter(Color::from_rgb(1.0, 0.0, 0.0), 110.0);
        assert_close(washed, Color::from_rgb(1.0, 0.1, 0.1), "washed");
    }
}
