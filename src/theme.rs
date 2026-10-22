//! The app's design tokens, derived from the desktop theme.
//!
//! [`build`] turns the resolved desktop theme into an [`AppTokens`] value that
//! carries every colour and metric the UI paints with, Breeze overrides
//! included. The tokens are toolkit-neutral; [`crate::ui`] copies them into
//! the Slint `Theme` global once at startup. The theme never changes at
//! runtime.

use native_theme::color::Rgba;
use native_theme::theme::{ResolvedFontSpec, ResolvedTheme};

use crate::system_theme::LoadedTheme;

/// A linear RGBA colour, each channel 0–1 in sRGB.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::from_rgb(1.0, 1.0, 1.0);

    pub const fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba8(r, g, b, 1.0)
    }

    pub fn from_rgba8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a,
        }
    }

    /// The colour as 8-bit channels, rounded.
    pub fn to_rgba8(self) -> [u8; 4] {
        let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            channel(self.a),
        ]
    }
}

impl From<Color> for slint::Color {
    fn from(color: Color) -> Self {
        let [r, g, b, a] = color.to_rgba8();
        slint::Color::from_argb_u8(a, r, g, b)
    }
}

/// `rgba` from the resolved theme.
fn to_color(rgba: Rgba) -> Color {
    let [r, g, b, a] = rgba.to_f32_array();
    Color { r, g, b, a }
}

/// Alpha of the KDE selection colour used for hover accents (menu items and
/// table rows).
pub const ACCENT_ALPHA: f32 = 0.3;

/// Alpha of the menu highlight's fill; the highlight is outlined in the
/// opaque colour (Breeze, as tuned in the egui rapidchecksum build).
const MENU_HOVER_ALPHA: f32 = 0.25;

/// Breeze's scroll bar handle: the focus colour, faded while idle and
/// hovered, opaque while dragged. `native-theme`'s scrollbar colours come from
/// the bundled preset whatever the KDE scheme says, so they are not used.
const THUMB_IDLE_ALPHA: f32 = 0.4;
const THUMB_HOVER_ALPHA: f32 = 0.65;

/// Breeze's scroll bar lane, across it: a separator line, a gap, the 9 px
/// handle and a slightly wider gap.
const SCROLLBAR_LINE: f32 = 1.0;
const SCROLLBAR_GAP: f32 = 6.0;
const SCROLLER_WIDTH: f32 = 9.0;
const SCROLLBAR_TRAILING_GAP: f32 = 7.0;

/// The lane separator: the view's grid colour at this alpha.
const SCROLLBAR_LINE_ALPHA: f32 = 0.7;

/// KDE's frame contrast: `[KDE] contrast=4` → Kirigami `frameContrast` 0.2.
/// QtWidgets' frame colour (`QPalette::Mid`) is the window text blended over
/// the window fill at this weight, in sRGB.
const FRAME_CONTRAST: f32 = 0.2;

/// Weight of the text in a table grid line: `QTableView`'s grid is the view
/// fill moved 4 % towards the text.
const GRID_CONTRAST: f32 = 0.04;

/// Every colour and metric the views read, resolved once from the desktop
/// theme.
#[derive(Clone, Debug)]
pub struct AppTokens {
    /// The desktop's reduced-motion preference (the spinner stands still).
    pub reduce_motion: bool,

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
    /// Hovered menu item: the selection tint, outlined in [`Self::menu_hover_border`].
    pub menu_hover: Color,
    pub menu_hover_border: Color,

    // ---- Scrollbars ----
    /// Lane fill (the view background).
    pub scrollbar_track: Color,
    /// Line between the content and a lane.
    pub scrollbar_separator: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_thumb_hover: Color,
    pub scrollbar_thumb_active: Color,
    /// Lane width, the separator line included.
    pub scrollbar_width: f32,
    /// Handle thickness.
    pub scroller_width: f32,
    /// Offset of the handle inside the lane.
    pub scrollbar_inset: f32,
    pub scrollbar_min_thumb: f32,

    // ---- Fonts ----
    pub font_family: String,
    pub font_weight: u16,
    pub font_size: f32,
    pub mono_family: String,
    pub mono_size: f32,
}

/// The tokens for `loaded`.
pub fn build(loaded: &LoadedTheme) -> AppTokens {
    tokens_for(loaded)
}

/// `size` scaled by the desktop's text-scaling preference (ignored when it is
/// not a finite positive factor).
fn scaled(size: f32, prefs: &native_theme::AccessibilityPreferences) -> f32 {
    let factor = prefs.text_scaling_factor;
    if factor.is_finite() && factor > 0.0 {
        size * factor
    } else {
        size
    }
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
    let mut saturation = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
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
    // Qt paints menu highlights with the Selection colour set, as list
    // selections are; `menu.hover_background` follows the accent instead.
    let menu_highlight = with_alpha(to_color(r.list.selection_background), 1.0);
    let ring = to_color(d.focus_ring_color);

    AppTokens {
        reduce_motion: loaded.prefs.reduce_motion,

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
        ring,
        accent: with_alpha(selection, ACCENT_ALPHA),
        selection,
        selection_hover: qt_lighter(primary, 110.0),
        danger: to_color(d.danger_color),
        warning: to_color(d.warning_color),
        radius: d.border.corner_radius.max(0.0),
        disabled_opacity: 0.5,

        menu_background: background,
        menu_foreground: to_color(r.menu.font.color),
        menu_hover: with_alpha(menu_highlight, MENU_HOVER_ALPHA),
        menu_hover_border: menu_highlight,

        scrollbar_track: view,
        scrollbar_separator: with_alpha(to_color(r.list.grid_color), SCROLLBAR_LINE_ALPHA),
        scrollbar_thumb: with_alpha(ring, THUMB_IDLE_ALPHA),
        scrollbar_thumb_hover: with_alpha(ring, THUMB_HOVER_ALPHA),
        scrollbar_thumb_active: with_alpha(ring, 1.0),
        scrollbar_width: SCROLLBAR_LINE + SCROLLBAR_GAP + SCROLLER_WIDTH + SCROLLBAR_TRAILING_GAP,
        scroller_width: SCROLLER_WIDTH,
        scrollbar_inset: SCROLLBAR_LINE + SCROLLBAR_GAP,
        scrollbar_min_thumb: r.scrollbar.min_thumb_length,

        font_family: family(&d.font),
        font_weight: d.font.weight,
        font_size: scaled(d.font.size, &loaded.prefs),
        mono_family: family(&d.mono_font),
        mono_size: scaled(d.mono_font.size, &loaded.prefs),
    }
}

/// The font family name of `spec`.
fn family(spec: &ResolvedFontSpec) -> String {
    spec.family.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        ACCENT_ALPHA, Color, MENU_HOVER_ALPHA, THUMB_HOVER_ALPHA, THUMB_IDLE_ALPHA, build,
        qt_lighter,
    };
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
        let t = build(&loaded);

        assert!(loaded.is_dark);
        assert_close(t.background, hex(0x202326), "window background");
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
        let focus = |a: f32| Color { a, ..hex(0x3DAEE9) };
        assert_close(
            t.scrollbar_thumb,
            focus(THUMB_IDLE_ALPHA),
            "scrollbar thumb",
        );
        assert_close(
            t.scrollbar_thumb_hover,
            focus(THUMB_HOVER_ALPHA),
            "thumb hover",
        );
        assert_close(t.scrollbar_thumb_active, focus(1.0), "thumb active");
        assert_close(t.scrollbar_track, hex(0x141618), "scrollbar lane");
        assert_close(t.menu_background, t.background, "menu background");
        assert_close(t.menu_hover, focus(MENU_HOVER_ALPHA), "menu hover");
        assert_close(t.menu_hover_border, focus(1.0), "menu hover outline");
        assert_eq!(t.scrollbar_width, 23.0);
        assert_eq!(t.scroller_width, 9.0);
        assert_eq!(t.scrollbar_inset, 7.0);
        assert_eq!(t.font_size, 10.0 * 96.0 / 72.0);
        assert_eq!(t.mono_size, 10.0 * 96.0 / 72.0);
        assert_eq!(t.font_family, "Noto Sans");
        assert_eq!(t.mono_family, "Hack");
    }

    #[test]
    fn breeze_dark_grid_is_close_to_the_pinned_shade() {
        let t = build(&preset(&Default::default(), true));
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
        let loaded = preset(&Default::default(), false);
        let t = build(&loaded);

        assert!(!loaded.is_dark);
        assert_close(t.background, hex(0xEFF0F1), "window background");
        // The lane is the view fill; the handle the focus colour, faded.
        assert_close(t.scrollbar_track, hex(0xFFFFFF), "scrollbar lane");
        assert_close(
            t.scrollbar_thumb,
            Color {
                a: THUMB_IDLE_ALPHA,
                ..hex(0x3DAEE9)
            },
            "scrollbar thumb",
        );
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
