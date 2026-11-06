//! A log file opened before anything else starts, so a launch that fails
//! still leaves a trace. Apps opened from Finder (or, later, a Windows GUI
//! build) have no terminal to read `eprintln!` from.
//!
//! Where it lives:
//! - macOS: `~/Library/Logs/Markdown Viewer/`
//! - Windows: `%LOCALAPPDATA%\Markdown Viewer\logs\`
//! - Linux: `$XDG_STATE_HOME/markdown-viewer/` (`~/.local/state/markdown-viewer/`)
//!
//! `MARKDOWN_VIEWER_LOG_DIR` overrides it. The previous launch's log is kept
//! as `markdown-viewer.old.log`. Everything also goes to stderr.

use std::{
    fs::File,
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const FILE: &str = "markdown-viewer.log";
const OLD: &str = "markdown-viewer.old.log";

pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MARKDOWN_VIEWER_LOG_DIR") {
        return Some(dir.into());
    }
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Logs/Markdown Viewer"))
    } else if cfg!(windows) {
        dirs::data_local_dir().map(|d| d.join("Markdown Viewer").join("logs"))
    } else {
        dirs::state_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
            .map(|d| d.join("markdown-viewer"))
    }
}

struct Logger {
    file: Mutex<Option<File>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("markdown_viewer")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}\n",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
        eprint!("{line}");
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// Open the log file and route `log` records and panics to it. Call first
/// thing in `main`. A log that can't be opened only loses the file copy.
pub fn init() {
    let file = dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(FILE);
        if path.exists() {
            let _ = std::fs::rename(&path, dir.join(OLD));
        }
        File::create(path).ok()
    });
    let logger = Box::leak(Box::new(Logger {
        file: Mutex::new(file),
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        });
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("{info}\n{backtrace}");
        log::logger().flush();
        default_hook(info);
    }));
    log::info!(
        "Markdown Viewer {} starting on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

/// UTC `YYYY-MM-DD HH:MM:SS.mmm`, without a date crate.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{:03}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        now.subsec_millis()
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_398), (2025, 11, 6));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
