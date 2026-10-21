//! The games table: `iced_table2` columns, header and body cells, and the
//! frame around them.
//!
//! `iced_table2` styles rows through a `Catalog` fixed for `iced::Theme`, so
//! every cell paints its own fill (cell padding and divider width are zero,
//! leaving no gaps for the library's row fill to show through) and the cells
//! read the view state the [`crate::app::State`] mirrors into each column
//! ([`TableView`]) after every update.

use iced::widget::{column, container, mouse_area, row, sensor, space, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use iced_aw::ContextMenu;
use iced_table2::table;

use crate::app::{Message, State};
use crate::assets::Icon;
use crate::games::{COLUMN_NAMES, ColumnSort, GameRow};
use crate::theme::tokens;
use crate::views::widgets::{self, icon, label};

pub const ROW_HEIGHT: f32 = 28.0;
/// Header height including its 1 px bottom rule.
pub const HEADER_HEIGHT: f32 = 28.0;
pub const NAME_WIDTH: f32 = 260.0;
pub const APP_ID_WIDTH: f32 = 110.0;
pub const MIN_COMPAT_WIDTH: f32 = 80.0;
pub const MIN_COLUMN_WIDTH: f32 = 40.0;
/// Horizontal text inset of a cell.
pub const CELL_PADDING_X: f32 = 8.0;
/// Padding around the header sort indicator (gpui `p(px(2.))`); the icon is
/// gpui's `size_3`, 0.75 rem.
const SORT_SLOT_PADDING: f32 = 2.0;

/// The view state every cell reads, mirrored from `State` after each update.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TableView {
    pub sort_column: usize,
    pub sort: ColumnSort,
    pub selected: Option<usize>,
    pub hovered: Option<usize>,
    pub focused: bool,
    pub cache_exists: bool,
}

/// One table column: its stored width, an in-progress resize, and the view
/// state and elided cell texts mirrored from `State`.
#[derive(Clone, Debug)]
pub struct GameColumn {
    pub index: usize,
    pub width: f32,
    pub resize_offset: Option<f32>,
    pub view: TableView,
    /// The column's text for each row, elided to the column width.
    pub texts: Vec<String>,
}

impl GameColumn {
    /// The width the column is drawn at, including an in-progress resize.
    pub fn current_width(&self) -> f32 {
        (self.width + self.resize_offset.unwrap_or(0.0)).max(MIN_COLUMN_WIDTH)
    }
}

pub fn initial_columns() -> [GameColumn; 3] {
    let column = |index, width| GameColumn {
        index,
        width,
        resize_offset: None,
        view: TableView::default(),
        texts: Vec::new(),
    };
    [
        column(0, NAME_WIDTH),
        column(1, APP_ID_WIDTH),
        column(2, MIN_COMPAT_WIDTH),
    ]
}

/// Only the Game and App ID columns resize; Compatibility Tool takes the
/// rest of the width.
pub fn is_resizable(column: usize) -> bool {
    column < 2
}

/// The compat column's width for a table `inner_width` wide whose fixed
/// columns take `fixed`: the rest, floored to whole pixels so the body never
/// overflows by a fraction, and never below [`MIN_COMPAT_WIDTH`].
pub fn compat_width(inner_width: f32, fixed: f32) -> f32 {
    (inner_width - fixed).floor().max(MIN_COMPAT_WIDTH)
}

/// `top` composited over the opaque `base`.
fn over(top: Color, base: Color) -> Color {
    let mix = |t: f32, b: f32| t * top.a + b * (1.0 - top.a);
    Color {
        r: mix(top.r, base.r),
        g: mix(top.g, base.g),
        b: mix(top.b, base.b),
        a: 1.0,
    }
}

/// A row's fill and text colour.
///
/// The selected row is filled with `primary` only while the main window is
/// focused; hovering it then lightens the fill (`highlight.lighter(110)`).
/// Any other hovered row takes the selection tint over its own fill.
pub fn row_colors(view: &TableView, row: usize) -> (Color, Color) {
    let t = tokens();
    let filled = view.focused && view.selected == Some(row);
    let hovered = view.hovered == Some(row);
    if filled {
        let fill = if hovered {
            t.selection_hover
        } else {
            t.primary
        };
        return (fill, t.primary_foreground);
    }
    let base = if row % 2 == 1 {
        t.alternate_row
    } else {
        t.list_background
    };
    let fill = if hovered { over(t.accent, base) } else { base };
    (fill, t.list_foreground)
}

impl<'a> table::Column<'a, Message, Theme, iced::Renderer> for GameColumn {
    type Row = GameRow;

    fn header(&'a self, col_index: usize) -> Element<'a, Message> {
        header_cell(col_index, &self.view)
    }

    fn cell(
        &'a self,
        col_index: usize,
        row_index: usize,
        _row: &'a GameRow,
    ) -> Element<'a, Message> {
        let text_value = self
            .texts
            .get(row_index)
            .map(String::as_str)
            .unwrap_or_default();
        body_cell(text_value, col_index, row_index, self.view)
    }

    fn width(&self) -> f32 {
        self.width
    }

    fn resize_offset(&self) -> Option<f32> {
        self.resize_offset
    }
}

/// One header cell: the centred, Medium-weight title with the sort
/// indicator slot, a grid line on its right, and the frame rule below. The
/// whole cell is the click target, like Qt's `QHeaderView`; the divider's
/// resize grip sits outside it and swallows presses near the edge.
fn header_cell<'a>(index: usize, view: &TableView) -> Element<'a, Message> {
    let t = tokens();
    let active = index == view.sort_column && view.sort != ColumnSort::Default;
    let indicator = match view.sort {
        ColumnSort::Descending => Icon::SortDescending,
        _ => Icon::SortAscending,
    };
    // Inactive columns keep an invisible slot of the same size so centred
    // titles do not shift when the sorted column changes.
    let slot =
        container(icon(indicator, t.font_size * 0.75).opacity(if active { 1.0 } else { 0.0 }))
            .padding(SORT_SLOT_PADDING);

    let title = text(COLUMN_NAMES.get(index).copied().unwrap_or_default())
        .font(t.header_font)
        .size(t.font_size)
        .width(Length::Fill)
        .align_x(Alignment::Center);

    // The compat column runs to the frame, but gpui sized it 16 px short
    // (the scrollbar rail) and centred its title in that width.
    let is_last = index == COLUMN_NAMES.len() - 1;
    let content = row![title, slot]
        .align_y(Alignment::Center)
        .padding(iced::Padding {
            left: CELL_PADDING_X - SORT_SLOT_PADDING,
            right: CELL_PADDING_X - SORT_SLOT_PADDING
                + if is_last { t.scrollbar_width } else { 0.0 },
            ..iced::Padding::ZERO
        });

    let is_first = index == 0;
    let label_area = container(content)
        .width(Length::Fill)
        .height(HEADER_HEIGHT - 1.0)
        .align_y(Alignment::Center)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.header)),
            text_color: Some(t.header_foreground),
            // The first cell rounds the frame's top-left corner so its fill
            // does not poke out of the rounded border.
            border: Border {
                radius: iced::border::Radius {
                    top_left: if is_first {
                        (t.radius - 1.0).max(0.0)
                    } else {
                        0.0
                    },
                    ..Default::default()
                },
                ..Border::default()
            },
            ..container::Style::default()
        });

    let grid_line = container(space())
        .width(1)
        .height(HEADER_HEIGHT - 1.0)
        .style(move |_theme: &Theme| solid(t.grid));
    let rule = container(space())
        .width(Length::Fill)
        .height(1)
        .style(move |_theme: &Theme| solid(t.border));

    let cell: Element<'a, Message> = if !is_last {
        column![row![label_area, grid_line], rule].into()
    } else {
        column![label_area, rule].into()
    };
    mouse_area(cell)
        .on_press(Message::HeaderPressed(index))
        .into()
}

/// A container style that is only a solid fill.
fn solid(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

/// One body cell: the elided text on the row's fill, the hover tracker, and
/// the row context menu.
fn body_cell<'a>(
    text_value: &'a str,
    column: usize,
    row: usize,
    view: TableView,
) -> Element<'a, Message> {
    let t = tokens();
    let (fill, foreground) = row_colors(&view, row);
    let content = container(
        label(text_value)
            .color(foreground)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fill)
    .height(ROW_HEIGHT - 1.0)
    // gpui centres row text 2 px lower than a plain centre in the
    // 27 px above the separator.
    .padding(iced::Padding {
        top: 4.0,
        right: CELL_PADDING_X,
        bottom: 0.0,
        left: CELL_PADDING_X,
    })
    .align_y(Alignment::Center)
    .clip(true)
    .style(move |_theme: &Theme| solid(fill));
    // Breeze separates rows with the alternating row shade, under every row
    // (the selected one included).
    let separator = container(space())
        .width(Length::Fill)
        .height(1)
        .style(move |_theme: &Theme| solid(t.alternate_row));
    let cell = column![content, separator];

    let tracked = mouse_area(cell)
        .on_enter(Message::CellEntered(row, column))
        .on_exit(Message::CellExited(row, column));

    ContextMenu::new(tracked, move || {
        crate::menu::row_context_menu(view.cache_exists)
    })
    .into()
}

/// The bordered table: header, rows, and the sensor that reports the inner
/// width the compat column fills.
pub fn view(state: &State) -> Element<'_, Message> {
    let t = tokens();
    let table = iced_table2::table(
        state.table_header.clone(),
        state.table_body.clone(),
        &state.columns,
        &state.rows,
        Message::TableSynced,
    )
    .on_row_press(Message::RowPressed)
    .on_column_resize(Message::ColumnDragged, Message::ColumnReleased)
    .min_column_width(MIN_COLUMN_WIDTH)
    .divider_width(0.0)
    .cell_padding(0)
    .scrollbar(widgets::scrollbar());

    let framed = container(table)
        .padding(1)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.list_background)),
            border: Border {
                color: t.border,
                width: 1.0,
                radius: t.radius.into(),
            },
            ..container::Style::default()
        });

    sensor(framed)
        .on_show(|size| Message::TableResized(inner(size)))
        .on_resize(|size| Message::TableResized(inner(size)))
        .into()
}

/// The table frame's inner size (inside its 1 px border).
fn inner(size: iced::Size) -> iced::Size {
    iced::Size::new((size.width - 2.0).max(0.0), (size.height - 2.0).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::{TableView, compat_width, row_colors};
    use crate::theme::tokens;

    #[test]
    fn compat_column_fills_the_rest_with_a_floor() {
        assert_eq!(compat_width(750.0, 370.0), 380.0);
        assert_eq!(compat_width(750.5, 370.0), 380.0);
        assert_eq!(compat_width(400.0, 370.0), 80.0);
    }

    #[test]
    fn selected_row_is_filled_only_while_focused() {
        let t = tokens();
        let mut view = TableView {
            selected: Some(1),
            focused: true,
            ..TableView::default()
        };
        assert_eq!(row_colors(&view, 1), (t.primary, t.primary_foreground));
        view.hovered = Some(1);
        assert_eq!(row_colors(&view, 1).0, t.selection_hover);

        view.focused = false;
        view.hovered = None;
        assert_eq!(row_colors(&view, 1).0, t.alternate_row);
        assert_eq!(row_colors(&view, 0).0, t.list_background);
    }
}
