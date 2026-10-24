//! Stage logging for playback diagnostics. A physical device often has no console attached to the app's stdio, so
//! when `LUNORX_PLAYBACK_LOG` is set the same lines are appended to `tmp/lunorx-playback.log` in the app's data
//! container, which `devicectl device copy from` reads over the developer connection.
//! Lines must never carry session cookies or signed stream URLs: hosts, sizes and status codes only.
use std::time::{SystemTime, UNIX_EPOCH};

pub fn line(msg: &str) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    eprintln!("[{stamp}] {msg}");
    if std::env::var_os("LUNORX_PLAYBACK_LOG").is_some() {
        use std::io::Write as _;
        let path = std::env::temp_dir().join("lunorx-playback.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "[{stamp}] {msg}");
        }
    }
}

macro_rules! stage_log {
    ($($arg:tt)*) => { $crate::core::playback::diag::line(&format!($($arg)*)) };
}
pub(crate) use stage_log;
