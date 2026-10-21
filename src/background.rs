//! Work that runs off the UI thread: the launch-output feed (pipe reader and
//! exit-watcher threads feeding a bounded channel, drained by a
//! subscription), the spinner tick, and the portal file chooser.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use iced::futures::{Stream, StreamExt, stream};

/// Backpressure cap for launch output events. The reader threads block when
/// the queue is full, which backpressures the child's pipe; without a bound,
/// a chatty process (or a stalled UI) would grow the queue forever.
pub const LAUNCH_EVENT_CAPACITY: usize = 1024;

/// Launch output is delivered in batches, one per ~16 ms window, so a chatty
/// process costs one update per frame rather than one per line.
const BATCH_WINDOW: Duration = Duration::from_millis(16);

/// The spinner's frame interval while a launch runs.
const SPINNER_TICK: Duration = Duration::from_millis(33);

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

/// The receiving end of the launch channel, as subscription data. The
/// subscription's identity is the feed itself, not the channel handle, so it
/// hashes to a constant: there is one feed for the app's lifetime.
#[derive(Clone)]
pub struct LaunchFeed(pub async_channel::Receiver<LaunchEvent>);

impl Hash for LaunchFeed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "launch-feed".hash(state);
    }
}

/// The launch events, batched: waits for the first event, lets the window
/// fill, then drains whatever arrived.
pub fn launch_batches(feed: &LaunchFeed) -> impl Stream<Item = Vec<LaunchEvent>> + use<> {
    stream::unfold(feed.0.clone(), |receiver| async move {
        let first = receiver.recv().await.ok()?;
        async_io::Timer::after(BATCH_WINDOW).await;
        let mut batch = vec![first];
        while let Ok(event) = receiver.try_recv() {
            batch.push(event);
        }
        Some((batch, receiver))
    })
}

/// Spinner frames while a launch runs.
pub fn spinner_ticks() -> impl Stream<Item = Instant> {
    stream::unfold((), |()| async {
        async_io::Timer::after(SPINNER_TICK).await;
        Some((Instant::now(), ()))
    })
    .boxed()
}

/// Stream one captured pipe (stdout or stderr) of a launched child to the
/// log, from its own thread: reads line by line (blocking until the child
/// closes the pipe) and forwards each line through `sender`.
pub fn stream_pipe<R: std::io::Read + Send + 'static>(
    reader: Option<R>,
    sender: async_channel::Sender<LaunchEvent>,
) {
    let Some(reader) = reader else {
        return;
    };

    let spawned = std::thread::Builder::new()
        .name("launch-output".to_string())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(reader).lines() {
                let text = match line {
                    Ok(text) => text,
                    Err(error) => {
                        let _ = sender.send_blocking(LaunchEvent::Line {
                            text: format!("read error: {error}"),
                        });
                        break;
                    }
                };
                if sender.send_blocking(LaunchEvent::Line { text }).is_err() {
                    break;
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("protonctx: could not start a launch output reader: {error}");
    }
}

/// Wait for `child` on a watcher thread and report its exit as
/// `generation`. The error is returned when the thread cannot be spawned; the
/// child then keeps running unwatched and the caller must clean up its
/// bookkeeping.
pub fn watch_exit(
    mut child: std::process::Child,
    generation: u64,
    sender: async_channel::Sender<LaunchEvent>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("launch-watcher".to_string())
        .spawn(move || {
            let result = child.wait().map_err(|error| error.to_string());
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
    use super::{LaunchEvent, LaunchFeed, exit_message, launch_batches};
    use iced::futures::StreamExt as _;

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
        let feed = LaunchFeed(receiver);
        let mut batches = Box::pin(launch_batches(&feed));
        let batch = iced::futures::executor::block_on(batches.next()).unwrap();
        assert_eq!(batch.len(), 5, "queued lines arrive as one batch");
    }
}
