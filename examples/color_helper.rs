//! GUI theme-colour catalogue.
//!
//! Opens a GPUI window that shows every colour definition exposed by
//! `native-theme` and `native-theme-gpui` as a labelled swatch (name + hex):
//!
//! - the bundled preset catalogue (`Theme::list_presets`);
//! - the resolved model (`ResolvedTheme`), grouped by top-level field;
//! - the `gpui-component` projection: `ThemeColor` (139 fields) and
//!   `ThemeTokens`;
//! - the gpui-base scrollbar mapping (`base_layer::scrollbar_geometry`).
//!
//! The window has a Light/Dark toggle and one chip per bundled preset; the
//! presets are re-resolved on selection, so every preset can be inspected
//! live. Colours come straight from the crate serializers, which emit
//! `#rrggbb` / `#rrggbbaa` strings.
//!
//! This is an inspection helper only: it lives outside `src/` and is not part
//! of the application.
//!
//! ```text
//! cargo run --example color_helper
//! cargo run --example color_helper -- --preset nord
//! cargo run --example color_helper -- --system
//! ```

use gpui_kit::component::{
    ActiveTheme as _, Theme as StyledTheme, h_flex, scroll::ScrollbarMode, try_parse_color, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use native_theme::theme::{ResolvedTheme, Theme as RawTheme};
use native_theme_gpui::{AccessibilityPreferences, SystemTheme};
use serde_json::Value;

fn main() {
    let options = parse_args();
    if !options.system
        && let Err(error) = RawTheme::preset(&options.preset)
    {
        eprintln!("unknown preset '{}' ({error})", options.preset);
        std::process::exit(2);
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);

            // Optional live KDE chrome so the window itself matches the
            // inspected theme; the catalogue always shows both variants.
            let system = options
                .system
                .then(|| SystemTheme::from_system().ok())
                .flatten();
            if let Some(system) = &system {
                native_theme_gpui::apply_system_theme(system, cx);
                StyledTheme::set_scrollbar_mode(ScrollbarMode::Always, cx);
            }

            let catalog_options = options.clone();
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(1100.), px(820.)), cx)),
                    window_min_size: Some(size(px(640.), px(400.))),
                    app_id: Some("protonctx-color-helper".to_string()),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Theme colour catalogue".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                move |_window, cx| {
                    cx.new(|_cx| ColorCatalog::new(&catalog_options, system.clone()))
                },
            )
            .expect("failed to open the colour-catalogue window");
        });
}

/// Command-line options; a GUI flag parser needs no dependency for these.
#[derive(Clone)]
struct Options {
    preset: String,
    system: bool,
}

fn parse_args() -> Options {
    let mut options = Options {
        preset: "kde-breeze".to_owned(),
        system: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--preset" => match args.next() {
                Some(name) => options.preset = name,
                None => {
                    eprintln!("--preset needs a preset key (see --help)");
                    std::process::exit(2);
                }
            },
            "--system" => options.system = true,
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument '{other}'");
                usage();
                std::process::exit(2);
            }
        }
    }
    options
}

fn usage() {
    println!(
        "GUI theme-colour catalogue\n\
         \n\
         USAGE:\n\
         \x20   cargo run --example color_helper [-- OPTIONS]\n\
         \n\
         OPTIONS:\n\
         \x20   --preset <key>   Preset shown at startup (default: kde-breeze)\n\
         \x20   --system         Start from the live system theme instead\n\
         \x20   -h, --help       Show this help"
    );
}

/// One rendered line in a group.
#[derive(Clone)]
enum Row {
    Swatch {
        name: String,
        hex: String,
        color: Hsla,
    },
    Text(String),
}

/// A titled block of rows; the scroll body is a flat list of groups.
#[derive(Clone)]
struct Group {
    title: String,
    rows: Vec<Row>,
}

/// The window view: the preset catalogue plus both resolved variants.
struct ColorCatalog {
    presets: Vec<(String, String)>,
    selected: String,
    display: String,
    dark_mode: bool,
    light: Vec<Group>,
    dark: Vec<Group>,
    system: Option<SystemTheme>,
    note: Option<String>,
}

impl ColorCatalog {
    fn new(options: &Options, system: Option<SystemTheme>) -> Self {
        let mut catalog = Self {
            presets: RawTheme::list_presets()
                .iter()
                .map(|info| (info.key.to_owned(), info.display_name.to_owned()))
                .collect(),
            selected: if system.is_some() {
                "system".to_owned()
            } else {
                options.preset.clone()
            },
            display: String::new(),
            dark_mode: false,
            light: Vec::new(),
            dark: Vec::new(),
            system,
            note: None,
        };
        if options.system && catalog.system.is_none() {
            catalog.note = Some(format!(
                "could not read the live system theme; showing {}",
                options.preset
            ));
        }
        catalog.rebuild();
        catalog
    }

    /// Re-resolves the selected source into swatch groups for both variants.
    fn rebuild(&mut self) {
        let prefs = AccessibilityPreferences::default();
        if self.selected == "system"
            && let Some(system) = &self.system
        {
            let name = system.name.to_string();
            let light = native_theme_gpui::to_theme(&system.light, &name, false, &prefs);
            let dark = native_theme_gpui::to_theme(&system.dark, &name, true, &prefs);
            self.light = catalogue(groups_for(&system.light, &light));
            self.dark = catalogue(groups_for(&system.dark, &dark));
            self.display = format!("{name} (live system)");
            return;
        }

        self.display = RawTheme::preset(&self.selected)
            .map(|theme| theme.name.to_string())
            .unwrap_or_else(|_| self.selected.clone());
        match native_theme_gpui::from_preset(&self.selected, false, &prefs) {
            Ok((theme, resolved)) => self.light = catalogue(groups_for(&resolved, &theme)),
            Err(_) => self.light = Vec::new(),
        }
        match native_theme_gpui::from_preset(&self.selected, true, &prefs) {
            Ok((theme, resolved)) => self.dark = catalogue(groups_for(&resolved, &theme)),
            Err(_) => self.dark = Vec::new(),
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let foreground = cx.theme().foreground;
        let border = cx.theme().border;
        let muted = cx.theme().muted;
        let muted_foreground = cx.theme().muted_foreground;
        let accent = cx.theme().accent;
        let accent_foreground = cx.theme().accent_foreground;

        let light_tab = div()
            .id("mode-light")
            .px_3()
            .py_1()
            .rounded(px(4.))
            .text_size(px(12.))
            .when(!self.dark_mode, |this| {
                this.bg(accent).text_color(accent_foreground)
            })
            .when(self.dark_mode, |this| this.bg(muted).text_color(foreground))
            .on_click(cx.listener(|this, _, _, cx| {
                this.dark_mode = false;
                cx.notify();
            }))
            .child("Light");
        let dark_tab = div()
            .id("mode-dark")
            .px_3()
            .py_1()
            .rounded(px(4.))
            .text_size(px(12.))
            .when(self.dark_mode, |this| {
                this.bg(accent).text_color(accent_foreground)
            })
            .when(!self.dark_mode, |this| {
                this.bg(muted).text_color(foreground)
            })
            .on_click(cx.listener(|this, _, _, cx| {
                this.dark_mode = true;
                cx.notify();
            }))
            .child("Dark");

        let mut chips = Vec::new();
        if self.system.is_some() {
            chips.push(preset_chip(
                "system",
                "System",
                self.selected == "system",
                cx,
            ));
        }
        for (key, display) in &self.presets {
            chips.push(preset_chip(key, display, *key == self.selected, cx));
        }

        v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(14.))
                            .child("Theme colour catalogue"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted_foreground)
                            .child(self.display.clone()),
                    )
                    .child(div().flex_1())
                    .child(light_tab)
                    .child(dark_tab),
            )
            .when_some(self.note.clone(), |this, note| {
                this.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(muted_foreground)
                        .child(note),
                )
            })
            .child(h_flex().w_full().gap_1().flex_wrap().children(chips))
    }
}

impl Render for ColorCatalog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let foreground = cx.theme().foreground;
        let toolbar = self.render_toolbar(cx);
        let groups = if self.dark_mode {
            &self.dark
        } else {
            &self.light
        };

        v_flex()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .child(toolbar)
            .child(
                div()
                    .id("catalog-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .px_3()
                            .pt_2()
                            .pb_6()
                            .children(groups.iter().map(|group| render_group(group, cx))),
                    ),
            )
    }
}

/// A preset chip that re-resolves the catalogue when clicked.
fn preset_chip(key: &str, label: &str, active: bool, cx: &mut Context<ColorCatalog>) -> AnyElement {
    let background = if active {
        cx.theme().accent
    } else {
        cx.theme().muted
    };
    let foreground = if active {
        cx.theme().accent_foreground
    } else {
        cx.theme().foreground
    };
    let border = cx.theme().border;
    let key = key.to_owned();
    let label = label.to_owned();

    div()
        .id(SharedString::from(format!("preset-{}", key)))
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(foreground)
        .text_size(px(11.5))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected = key.clone();
            this.note = None;
            this.rebuild();
            cx.notify();
        }))
        .child(label)
        .into_any_element()
}

fn render_group(group: &Group, cx: &App) -> AnyElement {
    let border = cx.theme().border;
    let muted_foreground = cx.theme().muted_foreground;

    v_flex()
        .w_full()
        .mb_4()
        .child(
            h_flex()
                .w_full()
                .h(px(28.))
                .items_center()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(12.5))
                        .child(group.title.clone()),
                ),
        )
        .children(group.rows.iter().map(|row| {
            match row {
                Row::Swatch { name, hex, color } => {
                    render_swatch(name, hex, *color, border, muted_foreground)
                }
                Row::Text(text) => div()
                    .py_0p5()
                    .text_size(px(11.5))
                    .text_color(muted_foreground)
                    .child(text.clone())
                    .into_any_element(),
            }
        }))
        .into_any_element()
}

fn render_swatch(
    name: &str,
    hex: &str,
    color: Hsla,
    border: Hsla,
    muted_foreground: Hsla,
) -> AnyElement {
    h_flex()
        .w_full()
        .h(px(22.))
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_shrink_0()
                .w(px(18.))
                .h(px(18.))
                .rounded(px(4.))
                .bg(color)
                .border_1()
                .border_color(border),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .child(name.to_owned()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(muted_foreground)
                .child(hex.to_owned()),
        )
        .into_any_element()
}

/// The preset catalogue is identical for both variants, so it is prepended to
/// each variant's group list.
fn catalogue(mut groups: Vec<Group>) -> Vec<Group> {
    groups.insert(0, preset_catalogue_group());
    groups
}

fn preset_catalogue_group() -> Group {
    let mut rows = Vec::new();
    for info in RawTheme::list_presets() {
        let variants = if info.light_only {
            "light only"
        } else {
            "light + dark"
        };
        let platforms = if info.platforms.is_empty() {
            "all platforms".to_owned()
        } else {
            info.platforms.join(", ")
        };
        rows.push(Row::Text(format!(
            "{:<20} {:<24} {:<12} {}",
            info.key, info.display_name, variants, platforms
        )));
    }
    Group {
        title: "native-theme bundled presets".to_owned(),
        rows,
    }
}

/// Every colour the stack exposes for one resolved variant.
fn groups_for(resolved: &ResolvedTheme, theme: &StyledTheme) -> Vec<Group> {
    let mut groups = Vec::new();
    push_json_groups(&mut groups, "ResolvedTheme", &to_value(resolved));
    push_json_groups(&mut groups, "ThemeColor", &to_value(&theme.colors));
    push_json_groups(&mut groups, "ThemeTokens", &to_value(&theme.tokens));
    groups.push(scrollbar_group(resolved));
    groups
}

/// `ThemeColor` is flat (one hex per field) and becomes a single group; every
/// other object becomes one group per top-level field, with only the hex
/// leaves collected into swatch rows.
fn push_json_groups(groups: &mut Vec<Group>, prefix: &str, value: &Value) {
    let Value::Object(map) = value else {
        return;
    };
    if map.values().all(is_hex) {
        let mut rows = Vec::new();
        for (key, entry) in map {
            if let Some(hex) = entry.as_str() {
                rows.push(swatch_row(key, hex));
            }
        }
        if !rows.is_empty() {
            groups.push(Group {
                title: prefix.to_owned(),
                rows,
            });
        }
        return;
    }
    for (key, entry) in map {
        let mut rows = Vec::new();
        let mut path = vec![key.clone()];
        collect_swatches(entry, &mut path, &mut rows);
        if !rows.is_empty() {
            groups.push(Group {
                title: format!("{prefix} :: {key}"),
                rows,
            });
        }
    }
}

fn collect_swatches(value: &Value, path: &mut Vec<String>, rows: &mut Vec<Row>) {
    match value {
        Value::String(hex) if is_hex(value) => rows.push(swatch_row(&path.join("."), hex)),
        Value::Object(map) => {
            for (key, entry) in map {
                path.push(key.clone());
                collect_swatches(entry, path, rows);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, entry) in items.iter().enumerate() {
                path.push(index.to_string());
                collect_swatches(entry, path, rows);
                path.pop();
            }
        }
        _ => {}
    }
}

fn scrollbar_group(resolved: &ResolvedTheme) -> Group {
    let g = native_theme_gpui::base_layer::scrollbar_geometry(resolved);
    let rows = vec![
        Row::Text(format!(
            "track_width = {:?}    thumb_width = {:?}    thumb_inset = {:?}",
            g.track_width, g.thumb_width, g.thumb_inset
        )),
        Row::Text(format!(
            "thumb_radius = {:?}    min_thumb_length = {:?}",
            g.thumb_radius, g.min_thumb_length
        )),
        hsla_row("track", g.track),
        hsla_row("track_active_border", g.track_active_border),
        hsla_row("thumb", g.thumb),
        hsla_row("thumb_hover", g.thumb_hover),
        hsla_row("thumb_active", g.thumb_active),
        Row::Text(
            "protonctx overrides: 16 px rail / 8 px thumb / 20 px min; \
             thumb = text @ 50%; hover and drag = selection background"
                .to_owned(),
        ),
    ];
    Group {
        title: "gpui-base scrollbar mapping (base_layer)".to_owned(),
        rows,
    }
}

fn swatch_row(name: &str, hex: &str) -> Row {
    Row::Swatch {
        name: name.to_owned(),
        hex: hex.to_owned(),
        color: try_parse_color(hex).unwrap_or_else(|_| hsla(0., 0., 0., 1.)),
    }
}

fn hsla_row(name: &str, color: Hsla) -> Row {
    Row::Swatch {
        name: name.to_owned(),
        hex: hsla_hex(color),
        color,
    }
}

fn is_hex(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.starts_with('#') && try_parse_color(text).is_ok())
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("theme models serialise to JSON")
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque, for a gpui colour.
fn hsla_hex(color: Hsla) -> String {
    let rgba: Rgba = color.into();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(rgba.a),
    );
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}
