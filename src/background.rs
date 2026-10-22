//! Work that runs off the UI thread: the launch-output feed (pipe reader and
//! exit-watcher threads feeding a bounded channel, drained in batches on the
//! UI event loop) and the portal file chooser.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

/// Backpressure cap for launch output events. The reader threads block when
/// the queue is full, which backpressures the child's pipe; without a bound,
/// a chatty process (or a stalled UI) would grow the queue forever.
pub const LAUNCH_EVENT_CAPACITY: usize = 1024;

/// Launch output is delivered in batches, one per ~16 ms window, so a chatty
/// process costs one update per frame rather than one per line.
const BATCH_WINDOW: Duration = Duration::from_millis(16);

/// Events sent from the launch reader/watcher threads to the UI.
#[derive(Clone, Debug)]
pub enum LaunchEvent {
    /// A captured stdout/stderr line, or a read error from one of the pipes.
    Line { text: String },
    /// A launched process exited (or waiting on it failed).
    Exited {
        generation: u64,
        result: Result<ExitStatus, String>,
    },
}

/// The log/status text for a process exit.
pub fn exit_message(result: &Result<ExitStatus, String>) -> String {
    match result {
        Ok(status) => match status.code() {
            Some(code) => format!("Exit code: {code}"),
            None => "Process terminated by signal".to_string(),
        },
        Err(error) => format!("Launch error: {error}"),
    }
}

/// The next batch of launch events: waits for the first event, lets the
/// window fill, then drains whatever arrived. `None` once every sender is
/// gone.
pub async fn next_batch(
    receiver: &async_channel::Receiver<LaunchEvent>,
) -> Option<Vec<LaunchEvent>> {
    let first = receiver.recv().await.ok()?;
    async_io::Timer::after(BATCH_WINDOW).await;
    let mut batch = vec![first];
    while let Ok(event) = receiver.try_recv() {
        batch.push(event);
    }
    Some(batch)
}

/// How long the exit watcher waits, after the child exits, for its pipe
/// readers to reach the end of the output, so the exit is reported after the
/// last lines. Bounded because the pipes can outlive the child: Wine's
/// wineserver inherits them and keeps them open after the launched tool
/// exits.
const READER_GRACE: Duration = Duration::from_millis(200);

/// Tracks a launch's pipe readers for [`watch_exit`]: each reader holds a
/// [`ReaderToken`] until it has forwarded its last line, and [`ReadersDone`]
/// sees every token dropped.
pub fn reader_tracker() -> (ReaderToken, ReadersDone) {
    let (sender, receiver) = std::sync::mpsc::channel();
    (ReaderToken(sender), ReadersDone(receiver))
}

/// Held by one pipe reader while it runs (cloned for each reader).
#[derive(Clone)]
pub struct ReaderToken(#[allow(dead_code)] std::sync::mpsc::Sender<()>);

/// Waits for every [`ReaderToken`] to be dropped.
pub struct ReadersDone(std::sync::mpsc::Receiver<()>);

impl ReadersDone {
    /// Block until every reader has finished, or `timeout` passes. Nothing is
    /// ever sent: the wait ends when the last token's channel side is dropped.
    fn wait(&self, timeout: Duration) {
        let _ = self.0.recv_timeout(timeout);
    }
}

/// Stream one captured pipe (stdout or stderr) of a launched child to the
/// log, from its own thread: reads line by line (blocking until the child
/// closes the pipe) and forwards each line through `sender`. `token` is
/// dropped once the last line has been sent (see [`watch_exit`]).
pub fn stream_pipe<R: std::io::Read + Send + 'static>(
    reader: Option<R>,
    sender: async_channel::Sender<LaunchEvent>,
    token: ReaderToken,
) {
    let Some(reader) = reader else {
        return;
    };

    let spawned = std::thread::Builder::new()
        .name("launch-output".to_string())
        .spawn(move || {
            let _token = token;
            let read = read_lines(reader, |text| {
                sender.send_blocking(LaunchEvent::Line { text }).is_ok()
            });
            if let Err(error) = read {
                let _ = sender.send_blocking(LaunchEvent::Line {
                    text: format!("read error: {error}"),
                });
            }
        });
    if let Err(error) = spawned {
        eprintln!("protonctx: could not start a launch output reader: {error}");
    }
}

/// Read `reader` to the end and pass each line to `emit`, stopping early
/// when `emit` returns `false`.
///
/// Lines end at `\n`, and a `\r\n` ending is trimmed whole, as
/// [`std::io::BufRead::lines`] does. Unlike `lines`, bytes that are not UTF-8
/// (a Windows program writing in a legacy code page) are decoded lossily
/// instead of failing: stopping would close the pipe's read end, losing the
/// rest of the output and breaking the child's next write. Only a real I/O
/// error ends the read early; `read_until` already retries interrupted reads.
fn read_lines(
    reader: impl std::io::Read,
    mut emit: impl FnMut(String) -> bool,
) -> std::io::Result<()> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        let line = match buf.strip_suffix(b"\n") {
            Some(line) => line.strip_suffix(b"\r").unwrap_or(line),
            None => &buf,
        };
        if !emit(String::from_utf8_lossy(line).into_owned()) {
            return Ok(());
        }
    }
}

/// Wait for `child` on a watcher thread and report its exit as
/// `generation`. The exit is sent once the pipe readers tracked by `readers`
/// have forwarded their last lines, or after [`READER_GRACE`] if a pipe stays
/// open, so it follows the child's output in the log. The error is returned
/// when the thread cannot be spawned; the child then keeps running unwatched
/// and the caller must clean up its bookkeeping.
pub fn watch_exit(
    mut child: std::process::Child,
    generation: u64,
    readers: ReadersDone,
    sender: async_channel::Sender<LaunchEvent>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("launch-watcher".to_string())
        .spawn(move || {
            let result = child.wait().map_err(|error| error.to_string());
            readers.wait(READER_GRACE);
            let _ = sender.send_blocking(LaunchEvent::Exited { generation, result });
        })
        .map(|_| ())
}

/// What the file chooser returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseOutcome {
    Picked(PathBuf),
    /// The user cancelled, or the response carried no usable local path.
    Nothing,
    Failed(String),
}

/// Prompt for a Windows executable through the XDG desktop portal.
pub async fn pick_executable(initial_dir: Option<PathBuf>) -> BrowseOutcome {
    match prompt_for_exe(initial_dir).await {
        Ok(Some(path)) => BrowseOutcome::Picked(path),
        // Portal cancel: no selection is not an error.
        Ok(None) | Err(ashpd::Error::Response(_)) => BrowseOutcome::Nothing,
        Err(error) => BrowseOutcome::Failed(error.to_string()),
    }
}

/// `Ok(None)` means the response carried no usable local path; a cancelled
/// dialog surfaces as `Err(ashpd::Error::Response(_))`.
async fn prompt_for_exe(initial_dir: Option<PathBuf>) -> ashpd::Result<Option<PathBuf>> {
    use ashpd::desktop::file_chooser::{FileFilter, OpenFileRequest};

    let request = OpenFileRequest::default()
        .title("Select executable to run")
        .multiple(false)
        .current_folder::<PathBuf>(initial_dir)?
        .filter(
            FileFilter::new("Windows executables")
                .glob("*.exe")
                .glob("*.EXE")
                .glob("*.bat")
                .glob("*.cmd"),
        )
        .filter(FileFilter::new("All Files").glob("*"))
        .send()
        .await?;
    let response = request.response()?;

    Ok(response
        .uris()
        .iter()
        .filter_map(|uri| url::Url::parse(uri.as_str()).ok())
        .find_map(|uri| uri.to_file_path().ok()))
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchEvent, exit_message, next_batch, read_lines, reader_tracker, stream_pipe, watch_exit,
    };

    /// Run `script` under `sh` with its stdout fed to the log, and collect the
    /// events until the exit.
    fn launch_events(script: &str) -> Vec<LaunchEvent> {
        let mut child = std::process::Command::new("sh")
            .args(["-c", script])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (sender, receiver) = async_channel::bounded(super::LAUNCH_EVENT_CAPACITY);
        let (token, readers) = reader_tracker();
        stream_pipe(child.stdout.take(), sender.clone(), token);
        watch_exit(child, 7, readers, sender).unwrap();
        let mut events = Vec::new();
        while let Ok(event) = receiver.recv_blocking() {
            let exited = matches!(event, LaunchEvent::Exited { .. });
            events.push(event);
            if exited {
                break;
            }
        }
        events
    }

    #[test]
    fn the_exit_follows_the_last_output_line() {
        // One write of ~9 KB just before exiting: it sits in the pipe while
        // the child is already reaped, the case the exit used to overtake.
        for _ in 0..20 {
            let events = launch_events("seq 1 2000");
            let lines = events
                .iter()
                .filter(|event| matches!(event, LaunchEvent::Line { .. }))
                .count();
            assert_eq!(lines, 2000, "every line arrives before the exit");
            assert!(matches!(
                events.last(),
                Some(LaunchEvent::Exited { generation: 7, .. })
            ));
        }
    }

    #[test]
    fn a_pipe_held_open_by_a_leftover_process_does_not_delay_the_exit() {
        // As wineserver does: a background process inherits stdout and
        // outlives the child.
        let started = std::time::Instant::now();
        let events = launch_events("sleep 3 & echo done");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(matches!(events.last(), Some(LaunchEvent::Exited { .. })));
    }

    #[test]
    fn exit_messages_match_the_status_strings() {
        use std::os::unix::process::ExitStatusExt as _;
        let ok = std::process::ExitStatus::from_raw(0);
        let failed = std::process::ExitStatus::from_raw(3 << 8);
        let signalled = std::process::ExitStatus::from_raw(9);
        assert_eq!(exit_message(&Ok(ok)), "Exit code: 0");
        assert_eq!(exit_message(&Ok(failed)), "Exit code: 3");
        assert_eq!(exit_message(&Ok(signalled)), "Process terminated by signal");
        assert_eq!(
            exit_message(&Err("no such process".to_string())),
            "Launch error: no such process"
        );
    }

    #[test]
    fn launch_events_arrive_in_batches() {
        let (sender, receiver) = async_channel::bounded(super::LAUNCH_EVENT_CAPACITY);
        for index in 0..5 {
            sender
                .send_blocking(LaunchEvent::Line {
                    text: format!("line {index}"),
                })
                .unwrap();
        }
        let batch = async_io::block_on(next_batch(&receiver)).unwrap();
        assert_eq!(batch.len(), 5, "queued lines arrive as one batch");
    }

    #[test]
    fn invalid_utf8_is_decoded_lossily_and_reading_continues() {
        // CP-1252 `é` is 0xE9: one bad line must not end the capture.
        let (sender, receiver) = async_channel::bounded(super::LAUNCH_EVENT_CAPACITY);
        stream_pipe(
            Some(std::io::Cursor::new(b"ok\n\xE9t\xE9\r\nlast".to_vec())),
            sender,
            reader_tracker().0,
        );
        let mut lines = Vec::new();
        // The reader thread drops the sender at EOF, which ends the loop.
        while let Ok(event) = receiver.recv_blocking() {
            match event {
                LaunchEvent::Line { text } => lines.push(text),
                LaunchEvent::Exited { .. } => panic!("no exit from a pipe reader"),
            }
        }
        assert_eq!(lines, ["ok", "\u{FFFD}t\u{FFFD}", "last"]);
    }

    #[test]
    fn line_endings_are_trimmed_like_lines() {
        let mut lines = Vec::new();
        read_lines(&b"a\r\nb\n\nc\r"[..], |line| {
            lines.push(line);
            true
        })
        .unwrap();
        // A lone `\r` without `\n` is kept, as `BufRead::lines` keeps it.
        assert_eq!(lines, ["a", "b", "", "c\r"]);
    }

    #[test]
    fn reading_stops_when_the_receiver_is_gone() {
        let mut lines = Vec::new();
        read_lines(&b"one\ntwo\nthree\n"[..], |line| {
            lines.push(line);
            lines.len() < 2
        })
        .unwrap();
        assert_eq!(lines, ["one", "two"]);
    }
}
