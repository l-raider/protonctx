//! The process log's text, capped to the newest lines.

/// Cap for the log; the oldest lines are trimmed first.
pub const MAX_LOG_LINES: usize = 500;
/// Extra lines allowed before a trim runs, so trimming is not per-line work.
pub const TRIM_SLACK: usize = 50;

/// The log as one string, every line terminated by `\n` so the newest line
/// sits above a blank row the horizontal scrollbar can overlay.
#[derive(Debug, Default)]
pub struct LogBuffer {
    text: String,
    lines: usize,
    /// Bumped on every change, so the UI knows when to copy the text.
    revision: u64,
    /// Lines trimmed from the top since the start, so the UI can keep a
    /// scrolled-up view on the same lines.
    trimmed: usize,
}

impl LogBuffer {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.lines
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn trimmed_total(&self) -> usize {
        self.trimmed
    }

    /// Append `message` as a `[HH:MM:SS.mmm] message` line. A message with
    /// line breaks (a host command's stderr) counts as every line it spans.
    /// Returns how many of the oldest lines a trim removed (0 when none ran).
    pub fn append(&mut self, message: &str) -> usize {
        let line = format!("[{}] {}", crate::log::timestamp(), message);
        self.push_line(&line)
    }

    fn push_line(&mut self, line: &str) -> usize {
        self.text.push_str(line);
        self.text.push('\n');
        self.lines += 1 + line.matches('\n').count();
        self.revision = self.revision.wrapping_add(1);
        if self.lines <= MAX_LOG_LINES + TRIM_SLACK {
            return 0;
        }
        let removed = self.lines - MAX_LOG_LINES;
        let cut = self
            .text
            .match_indices('\n')
            .nth(removed - 1)
            .map_or(self.text.len(), |(index, _)| index + 1);
        self.text.drain(..cut);
        self.lines = MAX_LOG_LINES;
        self.trimmed += removed;
        removed
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.lines = 0;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{LogBuffer, MAX_LOG_LINES, TRIM_SLACK};

    #[test]
    fn lines_are_timestamped_and_terminated() {
        let mut log = LogBuffer::default();
        assert_eq!(log.append("Refreshing games..."), 0);
        let text = log.text();
        assert!(text.starts_with('['), "{text}");
        assert!(text.ends_with("] Refreshing games...\n"), "{text}");
        // [HH:MM:SS.mmm]
        assert_eq!(text.find(']'), Some(13), "{text}");
        assert_eq!(log.line_count(), 1);
    }

    #[test]
    fn append_caps_line_count() {
        let mut log = LogBuffer::default();
        let total = MAX_LOG_LINES + TRIM_SLACK + 20;
        let mut removed = 0;
        for line in 0..total {
            removed += log.append(&format!("line {line}"));
        }
        let lines = log.line_count();
        assert!(
            (MAX_LOG_LINES..=MAX_LOG_LINES + TRIM_SLACK).contains(&lines),
            "{lines}"
        );
        assert_eq!(log.text().lines().count(), lines);
        assert_eq!(removed + lines, total);
        assert!(!log.text().contains("] line 0\n"));
        assert!(log.text().ends_with(&format!("] line {}\n", total - 1)));
    }

    #[test]
    fn multi_line_messages_count_every_line() {
        let mut log = LogBuffer::default();
        log.append("removing on the host failed:\nrm: denied a\nrm: denied b");
        assert_eq!(log.line_count(), 3);
        assert_eq!(log.text().lines().count(), 3);

        // The cap holds in physical lines too.
        let mut removed = 0;
        for index in 0..(MAX_LOG_LINES + TRIM_SLACK) {
            removed += log.append(&format!("message {index}\ncontinued {index}"));
        }
        let lines = log.line_count();
        assert!(lines <= MAX_LOG_LINES + TRIM_SLACK, "{lines}");
        assert_eq!(log.text().lines().count(), lines);
        assert_eq!(removed + lines, 3 + 2 * (MAX_LOG_LINES + TRIM_SLACK));
    }

    #[test]
    fn clear_empties_the_log() {
        let mut log = LogBuffer::default();
        log.append("seed");
        log.clear();
        assert!(log.text().is_empty());
        assert_eq!(log.line_count(), 0);
    }
}
