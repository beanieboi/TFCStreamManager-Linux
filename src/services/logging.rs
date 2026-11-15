//! The app log: every message goes to a log file (and the terminal). The
//! debug window tails that file instead of keeping its own copy.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

const FILE_NAME: &str = "tfc_stream_manager.log";
/// A larger log is moved to `.log.1` at startup, replacing the older one.
const ROTATE_BYTES: u64 = 5 * 1024 * 1024;
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// `logs/tfc_stream_manager.log` in the app's state (Linux) or data dir.
pub fn log_path() -> PathBuf {
    let dir = ProjectDirs::from("com", "benfritsch", "TFCStreamManager")
        .map(|dirs| {
            dirs.state_dir()
                .unwrap_or_else(|| dirs.data_local_dir())
                .join("logs")
        })
        .unwrap_or_else(|| std::env::temp_dir().join("TFCStreamManager"));
    dir.join(FILE_NAME)
}

/// The file always gets everything of the app (debug) and warnings of its
/// dependencies, whatever `RUST_LOG` says.
const FILE_FILTER: &str = "warn,tfc_stream_manager=debug";

/// Sets up logging to the log file ([`log_path`]) and the terminal. The
/// terminal follows `RUST_LOG`, `info` by default. Each line names the
/// module it comes from, so it can be filtered by it.
pub fn init() -> Result<()> {
    let path = log_path();
    let file = open(&path)?;
    let launch = launch_id();
    let terminal_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_timer(LaunchTime(launch.clone()))
                .with_filter(terminal_filter),
        )
        .with(
            fmt::layer()
                .with_writer(Mutex::new(file))
                .with_ansi(false)
                .with_timer(LaunchTime(launch.clone()))
                .with_filter(EnvFilter::new(FILE_FILTER)),
        )
        .init();
    tracing::info!(
        "App started: version {}, launch {launch}, log {}",
        env!("CARGO_PKG_VERSION"),
        path.display()
    );
    Ok(())
}

/// A short random id for this start of the app, written on every line, so
/// the lines of one run can be told apart (and grepped) from the others.
fn launch_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    format!("{:06x}", hasher.finish() & 0xff_ffff)
}

/// Local time, then the launch id: `2026-11-15 09:39:18 a1b2c3`.
struct LaunchTime(String);

impl FormatTime for LaunchTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{} {}", chrono::Local::now().format(TIME_FORMAT), self.0)
    }
}

fn open(path: &PathBuf) -> Result<File> {
    let dir = path.parent().context("log path has no folder")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    if fs::metadata(path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))
}

/// Severity of a log entry, as written in the file. Ordered, so a filter
/// can show a level and everything above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub const ALL: [Level; 5] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }
}

/// One line of the log file, split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `2026-11-15 09:30:56`
    pub timestamp: String,
    /// The app start the line belongs to; empty for lines written before
    /// launch ids existed.
    pub launch: String,
    pub level: Level,
    /// The module, e.g. `tfc_stream_manager::services::web_server`; empty
    /// when the line names none.
    pub source: String,
    pub message: String,
}

impl Entry {
    /// `09:30:56`
    pub fn time(&self) -> &str {
        self.timestamp
            .split_once(' ')
            .map_or(self.timestamp.as_str(), |(_, time)| time)
    }

    /// The module's last part, e.g. `web_server`.
    pub fn short_source(&self) -> &str {
        self.source.rsplit("::").next().unwrap_or(&self.source)
    }
}

/// Parses `2026-11-15 09:30:56 a1b2c3  INFO module::path: message` (the
/// launch id may be missing in older lines). `None` for a line that is not
/// an entry of its own (e.g. a continuation line).
pub fn parse_line(line: &str) -> Option<Entry> {
    let (date, rest) = line.split_once(' ')?;
    let (time, rest) = rest.split_once(' ')?;
    let valid_date = date.len() == 10 && date.as_bytes()[4] == b'-';
    let valid_time = time.len() == 8 && time.as_bytes()[2] == b':';
    if !valid_date || !valid_time {
        return None;
    }
    let rest = rest.trim_start();
    let (first, after) = rest.split_once(' ').unwrap_or((rest, ""));
    let (launch, level, rest) = match Level::parse(first) {
        Some(level) => (String::new(), level, after),
        None => {
            let after = after.trim_start();
            let (level, rest) = after.split_once(' ').unwrap_or((after, ""));
            (first.to_owned(), Level::parse(level)?, rest)
        }
    };
    let (source, message) = match rest.split_once(": ") {
        Some((source, message))
            if !source.is_empty()
                && source
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') =>
        {
            (source.to_owned(), message.to_owned())
        }
        _ => (String::new(), rest.to_owned()),
    };
    Some(Entry {
        timestamp: format!("{date} {time}"),
        launch,
        level,
        source,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_entry() {
        let e = parse_line(
            "2026-11-15 09:30:56  WARN tfc_stream_manager::services::web_server: GET /x → 404 Not Found (0 ms, localhost)",
        )
        .unwrap();
        assert_eq!(e.level, Level::Warn);
        assert_eq!(e.time(), "09:30:56");
        assert_eq!(e.short_source(), "web_server");
        assert_eq!(e.message, "GET /x → 404 Not Found (0 ms, localhost)");
    }

    #[test]
    fn a_message_with_a_colon_keeps_it() {
        let e = parse_line("2026-11-15 09:30:56 ERROR app::x: Failed: disk full").unwrap();
        assert_eq!(e.source, "app::x");
        assert_eq!(e.message, "Failed: disk full");
    }

    #[test]
    fn the_launch_id_follows_the_time() {
        let e = parse_line("2026-11-15 09:39:18 a1b2c3  INFO app::web: Started").unwrap();
        assert_eq!(e.launch, "a1b2c3");
        assert_eq!(e.level, Level::Info);
        assert_eq!(e.source, "app::web");
        let old = parse_line("2026-11-15 09:39:18  INFO app::web: Started").unwrap();
        assert_eq!(old.launch, "");
    }

    #[test]
    fn launch_ids_are_short_and_differ() {
        let (a, b) = (launch_id(), launch_id());
        assert_eq!(a.len(), 6);
        assert_ne!(a, b);
    }

    #[test]
    fn older_lines_without_a_module() {
        let e = parse_line("2026-11-15 09:26:03  INFO [WebServer] Started on port 8080").unwrap();
        assert_eq!(e.level, Level::Info);
        assert_eq!(e.source, "");
        assert_eq!(e.message, "[WebServer] Started on port 8080");
    }

    #[test]
    fn continuation_lines_are_not_entries() {
        assert_eq!(parse_line("    at some backtrace line"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("2026-11-15 09:30:56 LOUD x: y"), None);
    }

    #[test]
    fn levels_are_ordered() {
        assert!(Level::Debug < Level::Info && Level::Warn < Level::Error);
    }
}
