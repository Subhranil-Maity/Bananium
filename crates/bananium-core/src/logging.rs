//! The launcher's own log: one timestamped file per run under
//! [`Paths::logs_dir`], plus an optional stderr copy.
//!
//! Every line goes straight to the file with no buffering in between. The
//! release profile builds with `panic = "abort"`, so anything still sitting
//! in a buffer when a panic hits would be lost — and the panic line is the
//! one we most need. Log volume is low enough that unbuffered writes cost
//! nothing noticeable.
//!
//! How a run ended is read back from its file (see [`classify_tail`]): the
//! last line is [`SHUTDOWN_MARKER`] for a clean close, a [`PANIC_MARKER`]
//! line for a panic, and anything else means the process was killed or died
//! before it could say so.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::Paths;

/// Written by [`shutdown`] as the last line of a cleanly closed run.
pub const SHUTDOWN_MARKER: &str = "Bananium shutting down:";

/// Starts the message the panic hook writes.
pub const PANIC_MARKER: &str = "PANIC ";

/// How many launcher logs are kept; older ones are deleted at startup.
pub const KEEP_LOGS: usize = 50;

/// Target of the startup banner and the shutdown marker. Kept off stderr,
/// where a CLI user only wants the command's own output.
const LIFECYCLE_TARGET: &str = "bananium::lifecycle";

/// Target of the panic hook's line, so it can be told apart from an
/// ordinary message that happens to contain [`PANIC_MARKER`].
const PANIC_TARGET: &str = "bananium::panic";

static CURRENT: OnceLock<PathBuf> = OnceLock::new();
static FRONTEND: OnceLock<&'static str> = OnceLock::new();

/// Options a frontend passes to [`init`].
#[derive(Debug, Clone, Copy)]
pub struct LogOptions {
    /// Which frontend is running (`"desktop"`, `"cli"`, `"tui"`). Recorded in
    /// the startup banner, not in the file name.
    pub frontend: &'static str,
    /// Also write to stderr. Off for UIs that own the terminal (the TUI) or
    /// have none (the release desktop build).
    pub stderr: bool,
}

/// How a finished run ended, as read back from its log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndKind {
    /// The run logged [`SHUTDOWN_MARKER`].
    Closed,
    /// The run logged a panic.
    Crashed,
    /// Neither: killed, power loss, or a crash that never reached the hook.
    Unclean,
}

/// Initialize the global `tracing` subscriber for a binary frontend and
/// install the panic hook. Returns this run's log file, or `None` if it
/// couldn't be created (logging then falls back to stderr alone, if enabled).
///
/// `BANANIUM_LOG` overrides the filter. Without it, Bananium's own targets
/// log at `debug` in debug builds and `info` in release; everything else
/// (hyper, reqwest, tao, ...) is capped at `warn`.
pub fn init(paths: &Paths, opts: LogOptions) -> Option<PathBuf> {
    let dir = paths.logs_dir();
    let opened = std::fs::create_dir_all(&dir).ok().and_then(|()| {
        // Leave room for the file this run is about to create.
        prune(&dir, KEEP_LOGS.saturating_sub(1));
        create_log_file(&dir, &chrono::Local::now()).ok()
    });
    let (path, file) = opened.unzip();

    let file_layer = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .event_format(LineFormat)
            .with_writer(Mutex::new(file))
            .with_filter(filter())
    });
    let stderr_layer = opts.stderr.then(|| {
        // What stderr showed before there was a log file: `info` and up,
        // so a CLI user's terminal isn't flooded with debug detail.
        let filter = EnvFilter::try_from_env("BANANIUM_LOG")
            .unwrap_or_else(|_| EnvFilter::new(format!("info,{LIFECYCLE_TARGET}=warn")));
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_writer(io::stderr)
            .with_filter(filter)
    });
    if tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .is_err()
    {
        return None;
    }

    install_panic_hook();
    if let Some(path) = &path {
        let _ = CURRENT.set(path.clone());
    }
    let _ = FRONTEND.set(opts.frontend);
    tracing::info!(
        target: LIFECYCLE_TARGET,
        "Bananium {} starting: frontend={} os={} arch={} pid={} home={} log={}",
        env!("CARGO_PKG_VERSION"),
        opts.frontend,
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id(),
        paths.home().display(),
        path.as_deref().map_or_else(|| "<none>".into(), |p| p.display().to_string()),
    );
    path
}

/// This run's log file, once [`init`] has created it.
pub fn current_log_file() -> Option<&'static Path> {
    CURRENT.get().map(PathBuf::as_path)
}

/// The frontend this process passed to [`init`].
pub fn current_frontend() -> Option<&'static str> {
    FRONTEND.get().copied()
}

/// The frontend named in a log's startup banner (its first line), so runs
/// of one frontend can be told apart from another's.
pub fn banner_frontend(head: &str) -> Option<&str> {
    let first = head.lines().next()?;
    let rest = first.split_once(" frontend=")?.1;
    Some(rest.split(' ').next().unwrap_or(rest))
}

/// Log the clean-shutdown marker. Call it last, when the frontend is about
/// to exit on purpose; a run whose file doesn't end with it is reported as
/// having crashed or been killed.
pub fn shutdown(reason: &str) {
    tracing::info!(target: LIFECYCLE_TARGET, "{SHUTDOWN_MARKER} {reason}");
}

/// Classify how a run ended from the tail of its log file, with a one-line
/// reason suitable for showing to the user.
pub fn classify_tail(tail: &str) -> (EndKind, Option<String>) {
    let mut last_error = None;
    for line in tail.lines().rev().filter(|l| is_header(l)) {
        if let Some(rest) = line
            .split_once(&format!("[{LIFECYCLE_TARGET}] {SHUTDOWN_MARKER}"))
            .map(|(_, r)| r.trim())
        {
            return (EndKind::Closed, Some(format!("Closed: {rest}")));
        }
        if let Some(rest) = line
            .split_once(&format!("[{PANIC_TARGET}] {PANIC_MARKER}"))
            .map(|(_, r)| r.trim())
        {
            return (EndKind::Crashed, Some(format!("Crashed: {rest}")));
        }
        if last_error.is_none() {
            last_error = line.split_once(" [ERROR] ").map(|(_, r)| {
                // Drop the `[target] ` prefix.
                r.split_once("] ").map_or(r, |(_, m)| m).trim().to_string()
            });
        }
    }
    let base = "Closed unexpectedly (killed, power loss, or a crash before it could be logged)";
    let reason = match last_error {
        Some(err) => format!("{base}; last error: {err}"),
        None => base.to_string(),
    };
    (EndKind::Unclean, Some(reason))
}

/// Whether `line` starts a log record (rather than continuing one, like a
/// backtrace frame). Records start with `YYYY-MM-DD `.
fn is_header(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() > 11
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[..4].iter().all(u8::is_ascii_digit)
}

fn filter() -> EnvFilter {
    EnvFilter::try_from_env("BANANIUM_LOG").unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "warn,bananium=debug,webview=debug"
        } else {
            "warn,bananium=info,webview=info"
        })
    })
}

/// Create `<dir>/<timestamp>.log`, or `<timestamp>_1.log`, `_2`, ... if
/// that name is taken (two runs in the same second). `create_new` makes the
/// pick race-free between processes.
fn create_log_file(
    dir: &Path,
    now: &chrono::DateTime<chrono::Local>,
) -> io::Result<(PathBuf, File)> {
    let base = now.format("%Y-%m-%d_%H-%M-%S").to_string();
    for n in 0..1000 {
        let name = if n == 0 {
            format!("{base}.log")
        } else {
            format!("{base}_{n}.log")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free log file name",
    ))
}

/// Delete all but the `keep` most recently modified `*.log` files in `dir`.
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort_by(|a, b| b.cmp(a));
    for (_, path) in logs.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let location = info.location().map_or_else(
            || "<unknown location>".to_string(),
            |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
        );
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            target: PANIC_TARGET,
            "{PANIC_MARKER}{payload} (at {location}, thread '{}')\n{backtrace}",
            thread.name().unwrap_or("<unnamed>"),
        );
        previous(info);
    }));
}

/// `2026-09-26 14:30:05.123 [INFO] [target] message key=value`, in local
/// time. The desktop console's parser relies on this shape.
struct LineFormat;

impl<S, N> FormatEvent<S, N> for LineFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        write!(
            writer,
            "{} [{}] [{}] ",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            meta.level(),
            meta.target(),
        )?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    #[test]
    fn same_second_names_get_a_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::Local::now();
        let names: Vec<String> = (0..3)
            .map(|_| {
                let (p, _) = create_log_file(dir.path(), &now).unwrap();
                p.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect();
        let base = now.format("%Y-%m-%d_%H-%M-%S").to_string();
        assert_eq!(
            names,
            [
                format!("{base}.log"),
                format!("{base}_1.log"),
                format!("{base}_2.log")
            ]
        );
    }

    #[test]
    fn prune_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            let path = dir.path().join(format!("{i}.log"));
            std::fs::write(&path, "x").unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        std::fs::write(dir.path().join("keep.txt"), "x").unwrap();
        prune(dir.path(), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["3.log", "4.log", "keep.txt"]);
    }

    #[test]
    fn line_format() {
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl Write for Buf {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf(Arc::default());
        let writer = buf.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .event_format(LineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "bananium_api::x", count = 3, "hello");
        });
        let out = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(is_header(&out), "{out}");
        assert!(
            out.ends_with(" [WARN] [bananium_api::x] hello count=3\n"),
            "{out}"
        );
    }

    const T: &str = "2026-09-26 14:30:05.123";

    #[test]
    fn banner_names_the_frontend() {
        let head = format!(
            "{T} [INFO] [bananium] Bananium 0.1.0 starting: frontend=desktop os=windows
more"
        );
        assert_eq!(banner_frontend(&head), Some("desktop"));
        assert_eq!(banner_frontend("garbage"), None);
    }

    #[test]
    fn classify_closed() {
        let tail = format!(
            "{T} [INFO] [bananium_api] launched\n{T} [INFO] [{LIFECYCLE_TARGET}] {SHUTDOWN_MARKER} window closed\n"
        );
        assert_eq!(
            classify_tail(&tail),
            (EndKind::Closed, Some("Closed: window closed".into()))
        );
    }

    #[test]
    fn classify_crashed_ignores_backtrace_lines() {
        let tail = format!(
            "{T} [ERROR] [{PANIC_TARGET}] {PANIC_MARKER}boom (at src/a.rs:1:2, thread 'main')\n   0: std::backtrace\n   1: {SHUTDOWN_MARKER} not a header\n"
        );
        assert_eq!(
            classify_tail(&tail),
            (
                EndKind::Crashed,
                Some("Crashed: boom (at src/a.rs:1:2, thread 'main')".into())
            )
        );
    }

    #[test]
    fn classify_unclean_mentions_last_error() {
        let tail = format!(
            "{T} [ERROR] [bananium_api] install failed: disk full\n{T} [INFO] [bananium_api] still going\n"
        );
        let (kind, reason) = classify_tail(&tail);
        assert_eq!(kind, EndKind::Unclean);
        assert!(reason
            .unwrap()
            .ends_with("last error: install failed: disk full"));
        assert_eq!(classify_tail("").0, EndKind::Unclean);
    }
}
