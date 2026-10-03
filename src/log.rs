//! Log-pane helpers.

/// Current local time as `HH:mm:ss.zzz`, used as the prefix of every log line.
pub fn timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}
