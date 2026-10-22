//! Shared Breeze-style building blocks: themed text, icons that follow the
//! surrounding text colour, the outline button used by the action row and
//! every dialog, the transparent toolbar button, and the scrollbar style.
//!
//! Every visual value is read from the [`crate::theme`] token sidecar; the
//! `&Theme` passed to style closures is ignored.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::svg::{self as core_svg, Handle as SvgHandle};
use iced::advanced::widget::{self, Widget};
use iced::widget::{button, row, scrollable, text};
use iced::{Background, Border, Color, Element, Length, Rectangle, Size, Theme, mouse};

use crate::app::Message;
use crate::assets::{self, Icon};
use crate::theme::tokens;

/// Outline buttons are Qt-height (32 px) and at least 80 px wide.
pub const BUTTON_HEIGHT: f32 = 32.0;
pub const BUTTON_MIN_WIDTH: f32 = 80.0;
/// gpui's `px(14.)` inside its 1 px border.
const BUTTON_PADDING_X: f32 = 15.0;
/// Gap between a button's icon and its label.
const ICON_GAP: f32 = 4.0;

/// Text in the theme's UI font at the theme's base size. iced fixes its
/// default size when the renderer is created, so every text sets both.
pub fn label<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    let t = tokens();
    text(content).size(t.font_size).font(t.font)
}

/// `color` with its alpha scaled by `factor`.
pub fn faded(color: Color, factor: f32) -> Color {
    Color {
        a: color.a * factor,
        ..color
    }
}

/// An SVG icon drawn in the text colour of its parent (a button's label
/// colour, a menu item's, a container's), unless [`IconView::color`]
/// overrides it. iced's own `svg` widget only knows its own hover status, so
/// it cannot follow a hovered button's foreground.
pub struct IconView {
    handle: Option<SvgHandle>,
    size: f32,
    color: Option<Color>,
    opacity: f32,
}

/// `icon` at `size`×`size` logical pixels.
pub fn icon(icon: Icon, size: f32) -> IconView {
    IconView {
        handle: assets::svg_handle(icon),
        size,
        color: None,
        opacity: 1.0,
    }
}

impl IconView {
    /// Paints the icon in `color` instead of the inherited text colour.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Draws the icon at `opacity` (0 is an invisible, space-holding slot).
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for IconView
where
    Renderer: core_svg::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(self.size),
            Length::Fixed(self.size),
            Size::new(self.size, self.size),
        ))
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        if self.opacity <= 0.0 {
            return;
        }
        let bounds = layout.bounds();
        renderer.draw_svg(
            core_svg::Svg {
                handle: handle.clone(),
                color: Some(self.color.unwrap_or(style.text_color)),
                rotation: iced::Radians(0.0),
                opacity: self.opacity,
            },
            bounds,
            bounds.intersection(viewport).unwrap_or(bounds),
        );
    }
}

impl<'a, Message, Theme, Renderer> From<IconView> for Element<'a, Message, Theme, Renderer>
where
    Renderer: core_svg::Renderer + 'a,
{
    fn from(icon: IconView) -> Self {
        Element::new(icon)
    }
}

/// An optional leading icon plus a label, the content of every button.
pub fn icon_label<'a>(leading: Option<Icon>, content: &'a str) -> Element<'a, Message> {
    let t = tokens();
    match leading {
        Some(leading) => row![icon(leading, t.font_size), label(content)]
            .spacing(ICON_GAP)
            .align_y(iced::Alignment::Center)
            .into(),
        None => label(content).into(),
    }
}

/// The status to style an enabled-or-not button with.
///
/// iced 0.14 only learns a button's status from a redraw event and falls back
/// to `Disabled` until then; an open `iced_aw` menu overlay captures those
/// events, so enabled buttons beneath it would paint as disabled. A button
/// with an action is never disabled, so that fallback is mapped back.
pub fn effective_status(status: button::Status, enabled: bool) -> button::Status {
    match status {
        button::Status::Disabled if enabled => button::Status::Active,
        status => status,
    }
}

/// The Breeze outline button used by the action row and every dialog action:
/// an optional leading icon and a label, 32 px high, at least 80 px wide.
///
/// The width is computed from the measured label and rounded up to whole
/// pixels, so the 1 px outline lands on the pixel grid. `default_focus`
/// paints the focus border Qt shows on a dialog's default button; iced 0.14
/// buttons take no keyboard focus, so the dialog routes Enter/Space to the
/// default action itself.
pub fn outline_button<'a>(
    text_value: &'a str,
    leading: Option<Icon>,
    on_press: Option<Message>,
    default_focus: bool,
) -> Element<'a, Message> {
    let t = tokens();
    let icon_width = if leading.is_some() {
        t.font_size + ICON_GAP
    } else {
        0.0
    };
    let content_width = text_width(text_value, t.font, t.font_size) + icon_width;
    let enabled = on_press.is_some();
    let width = (content_width + 2.0 * BUTTON_PADDING_X)
        .ceil()
        .max(BUTTON_MIN_WIDTH);
    let button = button(
        iced::widget::container(icon_label(leading, text_value))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center),
    )
    .width(width)
    .height(BUTTON_HEIGHT)
    .padding(0)
    .style(move |_theme: &Theme, status| {
        outline_style(effective_status(status, enabled), default_focus)
    })
    .on_press_maybe(on_press);
    arrow_cursor(button)
}

/// The outline button's appearance in `status`.
pub fn outline_style(status: button::Status, default_focus: bool) -> button::Style {
    let t = tokens();
    let border = |color: Color| Border {
        color,
        width: 1.0,
        radius: t.radius.into(),
    };
    let base = button::Style {
        background: Some(Background::Color(t.input_background)),
        text_color: t.button_foreground,
        border: border(if default_focus {
            t.ring
        } else {
            t.input_border
        }),
        shadow: Default::default(),
        snap: true,
    };
    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            border: border(t.primary),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(t.accent)),
            border: border(t.primary),
            ..base
        },
        // Disabled: the whole control at half opacity, like the legacy row.
        button::Status::Disabled => button::Style {
            background: Some(Background::Color(faded(
                t.input_background,
                t.disabled_opacity,
            ))),
            text_color: faded(t.muted_foreground, t.disabled_opacity),
            border: border(faded(t.input_border, t.disabled_opacity)),
            ..base
        },
    }
}

/// The transparent toolbar button (hamburger and Refresh): gpui's medium
/// ghost button, 2 rem high. Hover and press fill with `primary` and switch
/// the label and icon to `primary_foreground`. `padding_x` is the side
/// padding; an icon-only button passes 0 and is laid out square.
pub fn toolbar_button<'a>(
    content: impl Into<Element<'a, Message>>,
    padding_x: f32,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let t = tokens();
    let height = t.rem(8.0);
    let mut inner = iced::widget::container(content)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
    if padding_x <= 0.0 {
        inner = inner.width(height).align_x(iced::Alignment::Center);
    }
    let enabled = on_press.is_some();
    arrow_cursor(
        button(inner)
            .height(height)
            .padding([0.0, padding_x])
            .on_press_maybe(on_press)
            .style(move |_theme: &Theme, status| toolbar_style(effective_status(status, enabled))),
    )
}

/// Wraps `content` so it never asks for the pointing-hand cursor.
///
/// iced's buttons and checkboxes request `Interaction::Pointer` while
/// hovered and offer no setting for it; Qt and KDE keep the arrow over
/// push buttons, menu items and check boxes, so the wrapper turns that
/// request into "no preference" (the default arrow). Everything else is
/// delegated unchanged, and the wrapper is transparent in the widget tree.
pub fn arrow_cursor<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(ArrowCursor {
        content: content.into(),
    })
}

struct ArrowCursor<'a> {
    content: Element<'a, Message>,
}

impl Widget<Message, Theme, iced::Renderer> for ArrowCursor<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<widget::Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut widget::Tree) {
        self.content.as_widget().diff(tree);
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        match self
            .content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
        {
            mouse::Interaction::Pointer => mouse::Interaction::None,
            interaction => interaction,
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

/// The toolbar button's appearance in `status`.
pub fn toolbar_style(status: button::Status) -> button::Style {
    let t = tokens();
    let base = button::Style {
        background: None,
        text_color: t.foreground,
        border: Border {
            radius: t.radius.into(),
            ..Border::default()
        },
        shadow: Default::default(),
        snap: true,
    };
    match status {
        button::Status::Active => base,
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(t.primary)),
            text_color: t.primary_foreground,
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: faded(t.foreground, t.disabled_opacity),
            ..base
        },
    }
}

/// Breeze scrollbar geometry: a 16 px rail with the 8 px handle centred in
/// it.
pub fn scrollbar() -> scrollable::Scrollbar {
    let t = tokens();
    scrollable::Scrollbar::new()
        .width(t.scrollbar_width)
        .scroller_width(t.scroller_width)
}

/// Breeze scrollbar colours: the window fill as the groove, the text at half
/// alpha as the resting handle, the selection while hovered or dragged.
pub fn scrollable_style(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let t = tokens();
    let (v_thumb, h_thumb) = match status {
        scrollable::Status::Active { .. } => (t.scrollbar_thumb, t.scrollbar_thumb),
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => {
            let hot = |hovered: bool| {
                if hovered {
                    t.scrollbar_thumb_hover
                } else {
                    t.scrollbar_thumb
                }
            };
            (
                hot(is_vertical_scrollbar_hovered),
                hot(is_horizontal_scrollbar_hovered),
            )
        }
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => {
            let grabbed = |dragged: bool| {
                if dragged {
                    t.scrollbar_thumb_active
                } else {
                    t.scrollbar_thumb
                }
            };
            (
                grabbed(is_vertical_scrollbar_dragged),
                grabbed(is_horizontal_scrollbar_dragged),
            )
        }
    };
    let rail = |thumb: Color| scrollable::Rail {
        background: Some(Background::Color(t.scrollbar_track)),
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(thumb),
            border: Border {
                radius: (t.scroller_width / 2.0).into(),
                ..Border::default()
            },
        },
    };
    let default = scrollable::default(_theme, status);
    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail(v_thumb),
        horizontal_rail: rail(h_thumb),
        gap: Some(Background::Color(t.scrollbar_track)),
        auto_scroll: default.auto_scroll,
    }
}

/// The laid-out width of `content` on one line, in `font` at `size`.
pub fn text_width(content: &str, font: iced::Font, size: f32) -> f32 {
    use iced::advanced::text::Paragraph as _;
    type Paragraph = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

    Paragraph::with_text(iced::advanced::Text {
        content,
        bounds: Size::INFINITE,
        size: size.into(),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
    .min_width()
}

/// `content` shortened with a trailing ellipsis until it fits `max_width`
/// (gpui's `text_ellipsis`; iced 0.14 text has no elision of its own).
pub fn elide(content: &str, max_width: f32, font: iced::Font, size: f32) -> String {
    const ELLIPSIS: &str = "…";
    if text_width(content, font, size) <= max_width {
        return content.to_string();
    }
    let chars: Vec<char> = content.chars().collect();
    // Longest prefix whose elided form fits: binary search over the char
    // count, since the width grows monotonically with the prefix.
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + ELLIPSIS;
        if text_width(&candidate, font, size) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return ELLIPSIS.to_string();
    }
    chars[..low]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_string()
        + ELLIPSIS
}

#[cfg(test)]
mod tests {
    use iced::{Element, Event, Point, Size, mouse};
    use iced_runtime::user_interface::{self, UserInterface};

    use crate::app::Message;

    /// The cursor `element` asks for with the pointer at `position`.
    fn cursor_over(element: Element<'_, Message>, position: Point) -> mouse::Interaction {
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 16.into());
        let mut ui = UserInterface::build(
            element,
            Size::new(200.0, 100.0),
            user_interface::Cache::default(),
            &mut renderer,
        );
        let mut messages = Vec::new();
        let (state, _) = ui.update(
            &[Event::Mouse(mouse::Event::CursorMoved { position })],
            mouse::Cursor::Available(position),
            &mut renderer,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        match state {
            user_interface::State::Updated {
                mouse_interaction, ..
            } => mouse_interaction,
            user_interface::State::Outdated => mouse::Interaction::None,
        }
    }

    #[test]
    fn buttons_keep_the_arrow_cursor() {
        let over = Point::new(10.0, 10.0);
        // iced's own button asks for the pointing hand...
        let plain: Element<'_, Message> = iced::widget::button("Refresh")
            .on_press(Message::Refresh)
            .into();
        assert_eq!(cursor_over(plain, over), mouse::Interaction::Pointer);
        // ...the app's buttons keep the default arrow, like Qt.
        let outline = super::outline_button("Explorer", None, Some(Message::Refresh), false);
        assert_eq!(cursor_over(outline, over), mouse::Interaction::None);
        let toolbar = super::toolbar_button(super::label("Refresh"), 10.0, Some(Message::Refresh));
        assert_eq!(cursor_over(toolbar, over), mouse::Interaction::None);
    }
}
