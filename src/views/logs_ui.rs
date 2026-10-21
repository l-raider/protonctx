//! The process-log pane: a framed, view-coloured, read-only monospace text
//! editor inside a scrollable.
//!
//! The editor is laid out at its full content height inside a vertical (and
//! horizontal, since lines never wrap) scrollable, so the scrollable owns the
//! viewport: it provides the Breeze scrollbars, reports the user's position,
//! and follows the newest line by staying snapped to the bottom until the
//! user scrolls up. Edits are dropped in [`LogState::perform`]; selection,
//! caret moves and copying still work.

use std::sync::{Arc, OnceLock};

use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset, Viewport};
use iced::widget::text_editor::{self, Action, Motion};
use iced::widget::{Id, container, operation, scrollable, sensor};
use iced::{Background, Border, Element, Length, Size, Task, Theme};

use crate::app::Message;
use crate::theme::tokens;
use crate::views::widgets;

/// Cap for the log; the oldest lines are trimmed first.
pub const MAX_LOG_LINES: usize = 500;
/// Extra lines allowed before a trim runs, so trimming is not per-line work.
pub const TRIM_SLACK: usize = 50;

/// Padding of the frame around the editor, and of the text inside it.
const FRAME_PADDING: f32 = 4.0;
/// The text inset inside the frame padding, matching the gpui textarea.
const EDITOR_PADDING: iced::Padding = iced::Padding {
    top: 8.0,
    right: 10.0,
    bottom: 8.0,
    left: 10.0,
};
/// iced's default relative line height.
const LINE_HEIGHT: f32 = 1.3;

/// The scrollable holding the editor, addressed by the follow and trim
/// scroll operations.
pub fn scroll_id() -> Id {
    Id::new("log-scroll")
}

/// The editor itself, focused by Ctrl+A.
pub fn editor_id() -> Id {
    Id::new("log-editor")
}

/// The log's text, the editor content mirroring it, and the viewport the
/// scrollable last reported.
pub struct LogState {
    pub content: text_editor::Content,
    /// Shadow of the content; every line, the last included, ends in `\n`,
    /// so the newest line sits above a blank row the horizontal scrollbar
    /// can overlay.
    text: String,
    lines: usize,
    /// True while the view is at the bottom; new lines keep it there.
    follow: bool,
    offset: AbsoluteOffset,
    /// The scrollable's viewport size, for the editor's minimum width.
    viewport: Size,
    /// Longest line, in characters, for the editor's no-wrap width.
    longest_line: usize,
}

impl Default for LogState {
    fn default() -> Self {
        Self::new()
    }
}

impl LogState {
    pub fn new() -> Self {
        Self {
            content: text_editor::Content::new(),
            text: String::new(),
            lines: 0,
            follow: true,
            offset: AbsoluteOffset { x: 0.0, y: 0.0 },
            viewport: Size::ZERO,
            longest_line: 0,
        }
    }

    /// The shadow text: every line, each ending in `\n`.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.lines
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Append `message` as a `[HH:MM:SS.mmm] message` line, trimming to
    /// [`MAX_LOG_LINES`] once [`TRIM_SLACK`] extra lines have built up. The
    /// returned task keeps the view at the bottom when it was following, or
    /// keeps the user's position across a trim.
    pub fn append(&mut self, message: &str) -> Task<Message> {
        let line = format!("[{}] {}", crate::log::timestamp(), message);
        self.push_line(&line)
    }

    fn push_line(&mut self, line: &str) -> Task<Message> {
        self.text.push_str(line);
        self.text.push('\n');
        self.lines += 1;
        self.longest_line = self.longest_line.max(line.chars().count());

        // Insert at the end without disturbing the user's caret or
        // selection: park the caret at the end, paste, then put it back.
        let saved = self.content.cursor();
        self.content.perform(Action::Move(Motion::DocumentEnd));
        self.content
            .perform(Action::Edit(text_editor::Edit::Paste(Arc::new(format!("{line}\n")))));
        self.content.move_to(saved);

        if self.lines > MAX_LOG_LINES + TRIM_SLACK {
            return self.trim();
        }
        self.follow_task()
    }

    /// Drop the oldest lines down to [`MAX_LOG_LINES`].
    fn trim(&mut self) -> Task<Message> {
        let removed = self.lines - MAX_LOG_LINES;
        let cut = self
            .text
            .match_indices('\n')
            .nth(removed - 1)
            .map_or(self.text.len(), |(index, _)| index + 1);
        self.text.drain(..cut);
        self.lines = MAX_LOG_LINES;
        self.longest_line = self.text.lines().map(|line| line.chars().count()).max().unwrap_or(0);
        self.content = text_editor::Content::with_text(&self.text);

        if self.follow {
            return self.follow_task();
        }
        // Keep the same lines in view: the content above moved up by the
        // removed lines.
        let shift = removed as f32 * line_height();
        let offset = AbsoluteOffset {
            x: self.offset.x,
            y: (self.offset.y - shift).max(0.0),
        };
        self.offset = offset;
        operation::scroll_to(scroll_id(), offset)
    }

    /// Snap the viewport to the bottom while following. iced keeps a
    /// relative offset of 1.0 pinned to the end as the content grows, until
    /// the user scrolls.
    fn follow_task(&self) -> Task<Message> {
        if !self.follow {
            return Task::none();
        }
        let x = if self.content_width() > self.viewport.width && self.viewport.width > 0.0 {
            self.offset.x / (self.content_width() - self.viewport.width)
        } else {
            0.0
        };
        operation::snap_to(scroll_id(), RelativeOffset { x, y: 1.0 })
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.lines = 0;
        self.longest_line = 0;
        self.content = text_editor::Content::new();
        self.follow = true;
    }

    /// Apply an editor action unless it edits: the log is read-only.
    pub fn perform(&mut self, action: Action) {
        if !action.is_edit() {
            self.content.perform(action);
        }
    }

    /// Select everything (Ctrl+A); the returned task focuses the editor so
    /// the selection is painted and Ctrl+C copies it.
    pub fn select_all(&mut self) -> Task<Message> {
        self.content.perform(Action::SelectAll);
        operation::focus(editor_id())
    }

    /// The selected text, if any.
    pub fn selection(&self) -> Option<String> {
        self.content.selection().filter(|text| !text.is_empty())
    }

    /// Track the user's position: at the bottom the log follows new lines,
    /// anywhere above it holds still.
    pub fn scrolled(&mut self, viewport: Viewport) {
        self.offset = viewport.absolute_offset();
        let bottom = viewport.absolute_offset().y + viewport.bounds().height;
        self.follow = bottom >= viewport.content_bounds().height - 1.0;
    }

    pub fn resized(&mut self, size: Size) {
        self.viewport = size;
    }

    /// The editor's width: the viewport, or the longest line where that is
    /// wider (lines never wrap, so the scrollable scrolls sideways).
    pub fn content_width(&self) -> f32 {
        let text = self.longest_line as f32 * mono_advance()
            + EDITOR_PADDING.left
            + EDITOR_PADDING.right
            + 2.0;
        text.max(self.viewport.width).floor()
    }
}

/// The advance of one monospace glyph, measured once.
fn mono_advance() -> f32 {
    static ADVANCE: OnceLock<f32> = OnceLock::new();
    *ADVANCE.get_or_init(|| {
        let t = tokens();
        widgets::text_width(&"0".repeat(64), t.mono_font, t.mono_size) / 64.0
    })
}

/// The height of one log line.
fn line_height() -> f32 {
    tokens().mono_size * LINE_HEIGHT
}

/// The framed log pane.
pub fn view(log: &LogState) -> Element<'_, Message> {
    let t = tokens();
    let editor = iced::widget::text_editor(&log.content)
        .id(editor_id())
        .on_action(Message::LogAction)
        .font(t.mono_font)
        .size(t.mono_size)
        .line_height(LINE_HEIGHT)
        .wrapping(iced::widget::text::Wrapping::None)
        .padding(EDITOR_PADDING)
        .width(log.content_width())
        .height(Length::Shrink)
        .style(|_theme: &Theme, _status| text_editor::Style {
            background: Background::Color(iced::Color::TRANSPARENT),
            border: Border::default(),
            placeholder: t.muted_foreground,
            value: t.foreground,
            selection: t.selection,
        });

    let scroll = scrollable(editor)
        .id(scroll_id())
        .direction(scrollable::Direction::Both {
            vertical: widgets::scrollbar(),
            horizontal: widgets::scrollbar(),
        })
        .on_scroll(Message::LogScrolled)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(widgets::scrollable_style);

    let measured = sensor(scroll)
        .on_show(Message::LogResized)
        .on_resize(Message::LogResized);

    container(measured)
        .padding(FRAME_PADDING)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(t.muted)),
            border: Border {
                color: t.border,
                width: 1.0,
                radius: t.radius.into(),
            },
            ..container::Style::default()
        })
        .into()
}
