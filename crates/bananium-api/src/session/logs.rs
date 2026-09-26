//! Reading logs so a frontend can show them without touching the
//! filesystem itself: the per-launch files `Session::launch` redirects a
//! game's stdout/stderr into, and the launcher's own per-run logs (see
//! `bananium_core::logging`).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use bananium_core::logging::{self, EndKind};

use super::{blocking, Session};
use crate::error::{Error, Result};
use crate::output::{CommandOutput, LauncherLogFile, LogFile, LogStatus};

/// Upper bound on one `LogRead` chunk. A chatty modded game can write
/// megabytes; the frontend polls, so each poll returns at most this much
/// and the next poll picks up from `next_offset`.
const MAX_CHUNK: u64 = 256 * 1024;

impl Session {
    /// Every launch log for `instance`, newest first.
    fn log_files(&self, instance: &str) -> Result<Vec<LogFile>> {
        self.instances().resolve(Some(instance))?;
        let dir = self.paths.instance_logs_dir(instance);
        let mut logs = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(logs);
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".log") {
                continue;
            }
            let meta = entry.metadata()?;
            let modified_unix = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or_default();
            logs.push(LogFile {
                name,
                path: entry.path(),
                size: meta.len(),
                modified_unix,
            });
        }
        logs.sort_by(|a, b| {
            b.modified_unix
                .cmp(&a.modified_unix)
                .then(b.name.cmp(&a.name))
        });
        Ok(logs)
    }

    /// `Command::LogList`.
    pub(super) fn log_list(&self, instance: &str) -> Result<CommandOutput> {
        Ok(CommandOutput::LogListed {
            instance: instance.to_string(),
            logs: self.log_files(instance)?,
        })
    }

    /// `Command::LogRead`: up to [`MAX_CHUNK`] bytes of `file` (the newest
    /// log when `None`) starting at `offset`. An `offset` of 0 on a file
    /// already larger than one chunk starts at its tail instead, so opening
    /// a huge old log shows the end (where a crash is) immediately.
    pub(super) fn log_read(
        &self,
        instance: &str,
        file: Option<&str>,
        offset: u64,
    ) -> Result<CommandOutput> {
        let logs = self.log_files(instance)?;
        let log = match file {
            Some(name) => logs
                .into_iter()
                .find(|l| l.name == name)
                .ok_or_else(|| Error::LogNotFound(name.to_string()))?,
            None => match logs.into_iter().next() {
                Some(log) => log,
                None => {
                    return Ok(CommandOutput::LogChunk {
                        file: None,
                        text: String::new(),
                        next_offset: 0,
                    })
                }
            },
        };

        let mut f = std::fs::File::open(&log.path)?;
        let len = f.metadata()?.len();
        let start = if offset == 0 && len > MAX_CHUNK {
            len - MAX_CHUNK
        } else {
            // A file shorter than `offset` was truncated/replaced; restart.
            if offset > len {
                0
            } else {
                offset
            }
        };
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        f.take(MAX_CHUNK).read_to_end(&mut buf)?;
        Ok(CommandOutput::LogChunk {
            file: Some(log.name),
            next_offset: start + buf.len() as u64,
            text: String::from_utf8_lossy(&buf).into_owned(),
        })
    }
}

/// How much of a finished launcher log's end is read to tell how that run
/// ended: enough for a panic line plus its backtrace.
const STATUS_TAIL: u64 = 64 * 1024;

impl Session {
    /// `Command::LauncherLogList`. Classifying each file reads its tail, so
    /// this runs on the blocking pool rather than stalling the runtime.
    pub(super) async fn launcher_log_list(&self) -> Result<CommandOutput> {
        let dir = self.paths.logs_dir();
        let logs = blocking(move || launcher_log_files(&dir)).await?;
        Ok(CommandOutput::LauncherLogListed {
            current: logging::current_log_file().map(file_name),
            logs,
        })
    }

    /// `Command::LauncherLogRead`: see the command's docs for the three
    /// ways `offset`/`before` select a chunk.
    pub(super) async fn launcher_log_read(
        &self,
        file: Option<String>,
        offset: u64,
        before: Option<u64>,
    ) -> Result<CommandOutput> {
        let path = match file {
            // Only names that are actually listed, so a crafted name can't
            // reach outside the logs directory.
            Some(name) => launcher_log_paths(&self.paths.logs_dir())?
                .into_iter()
                .find(|p| file_name(p) == name)
                .ok_or(Error::LogNotFound(name))?,
            None => match logging::current_log_file() {
                Some(p) => p.to_path_buf(),
                None => {
                    return Ok(CommandOutput::LauncherLogChunk {
                        file: None,
                        text: String::new(),
                        start: 0,
                        end: 0,
                        size: 0,
                    })
                }
            },
        };
        let name = file_name(&path);
        let chunk = blocking(move || read_lines(&path, offset, before)).await?;
        Ok(CommandOutput::LauncherLogChunk {
            file: Some(name),
            text: chunk.text,
            start: chunk.start,
            end: chunk.end,
            size: chunk.size,
        })
    }

    /// `Command::LauncherLastSession`: the newest launcher log, other than
    /// this run's own, written by the same frontend (so a CLI command run
    /// alongside the desktop app is never mistaken for its last session),
    /// classified from its tail. A crash or unclean exit is also noted in
    /// this run's log, so the two can be read side by side.
    pub(super) async fn launcher_last_session(&self) -> Result<CommandOutput> {
        let dir = self.paths.logs_dir();
        let log = blocking(move || {
            let current = logging::current_log_file();
            let frontend = logging::current_frontend();
            for path in launcher_log_paths(&dir)? {
                if Some(path.as_path()) == current {
                    continue;
                }
                let head = read_head(&path, 1024)?;
                if frontend.is_none() || logging::banner_frontend(&head) == frontend {
                    return describe(&path, current).map(Some);
                }
            }
            Ok(None)
        })
        .await?;
        if let Some(log) = &log {
            if matches!(log.status, LogStatus::Crashed | LogStatus::Unclean) {
                tracing::warn!(
                    "previous session {} did not close properly: {}",
                    log.name,
                    log.reason.as_deref().unwrap_or("unknown reason"),
                );
            }
        }
        Ok(CommandOutput::LauncherLastSession { log })
    }

    /// `Command::LogFrontend`.
    pub(super) fn log_frontend(&self, level: &str, message: &str) -> Result<CommandOutput> {
        // A runaway frontend error (a huge serialized object) shouldn't
        // bloat the log.
        let message: String = message.chars().take(8 * 1024).collect();
        match level {
            "error" => tracing::error!(target: "webview", "{message}"),
            "warn" => tracing::warn!(target: "webview", "{message}"),
            "debug" => tracing::debug!(target: "webview", "{message}"),
            _ => tracing::info!(target: "webview", "{message}"),
        }
        Ok(CommandOutput::Logged)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn modified_unix(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Every `*.log` in the launcher logs directory, newest first.
fn launcher_log_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut logs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "log") {
            logs.push((modified_unix(&entry.metadata()?), path));
        }
    }
    logs.sort_by(|a, b| b.cmp(a));
    Ok(logs.into_iter().map(|(_, p)| p).collect())
}

fn launcher_log_files(dir: &Path) -> Result<Vec<LauncherLogFile>> {
    let current = logging::current_log_file();
    launcher_log_paths(dir)?
        .iter()
        .map(|p| describe(p, current))
        .collect()
}

/// List entry for one launcher log. `current` is this run's own, which is
/// still running and so isn't classified.
fn describe(path: &Path, current: Option<&Path>) -> Result<LauncherLogFile> {
    let meta = std::fs::metadata(path)?;
    let (status, reason) = if Some(path) == current {
        (LogStatus::Running, None)
    } else {
        let tail = read_tail(path, STATUS_TAIL)?;
        let (kind, reason) = logging::classify_tail(&tail);
        let status = match kind {
            EndKind::Closed => LogStatus::Closed,
            EndKind::Crashed => LogStatus::Crashed,
            EndKind::Unclean => LogStatus::Unclean,
        };
        (status, reason)
    };
    Ok(LauncherLogFile {
        name: file_name(path),
        path: path.to_path_buf(),
        size: meta.len(),
        modified_unix: modified_unix(&meta),
        status,
        reason,
    })
}

/// The last `max` bytes of `path` (lossily decoded; a cut first line is
/// harmless to [`logging::classify_tail`], which only looks at headers).
fn read_tail(path: &Path, max: u64) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The first `max` bytes of `path`, lossily decoded.
fn read_head(path: &Path, max: u64) -> Result<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(max).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// One chunk of a text file, trimmed to whole lines.
struct LineChunk {
    text: String,
    start: u64,
    end: u64,
    size: u64,
}

/// Read at most [`MAX_CHUNK`] bytes of `path`, trimmed to whole lines so a
/// chunk never splits a line (or a UTF-8 character) in two:
///
/// - `before: Some(n)`: the chunk ending at `n` (paging backwards);
/// - `offset: 0`: the file's tail;
/// - otherwise: from `offset` on (following the file). A line still being
///   written is left for the next read.
///
/// A single line longer than a whole chunk is returned as-is rather than
/// never at all.
fn read_lines(path: &Path, offset: u64, before: Option<u64>) -> Result<LineChunk> {
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let (start, end) = match before {
        Some(b) => {
            let end = b.min(size);
            (end.saturating_sub(MAX_CHUNK), end)
        }
        None if offset == 0 => (size.saturating_sub(MAX_CHUNK), size),
        None => {
            // A file shorter than `offset` was truncated/replaced; restart.
            let start = if offset > size { 0 } else { offset };
            (start, size.min(start + MAX_CHUNK))
        }
    };
    let starts_mid_line = start > 0 && (before.is_some() || offset == 0);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.take(end - start).read_to_end(&mut buf)?;

    let mut first = 0;
    if starts_mid_line {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            first = i + 1;
        }
    }
    let last = match buf[first..].iter().rposition(|&b| b == b'\n') {
        Some(i) => first + i + 1,
        None if buf.len() as u64 >= MAX_CHUNK => buf.len(),
        None => first,
    };
    Ok(LineChunk {
        text: String::from_utf8_lossy(&buf[first..last]).into_owned(),
        start: start + first as u64,
        end: start + last as u64,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lines(dir: &Path, n: usize) -> (PathBuf, Vec<String>) {
        let path = dir.join("x.log");
        let lines: Vec<String> = (0..n)
            .map(|i| format!("line {i:06} {}", "x".repeat(90)))
            .collect();
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(&path, text).unwrap();
        (path, lines)
    }

    #[test]
    fn small_file_reads_whole_then_follows() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = write_lines(dir.path(), 3);
        let c = read_lines(&path, 0, None).unwrap();
        assert_eq!((c.start, c.end), (0, c.size));
        assert_eq!(c.text.lines().count(), 3);

        // A partial trailing line waits for the next read.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut f, b"new line\npart").unwrap();
        let next = read_lines(&path, c.end, None).unwrap();
        assert_eq!(next.text, "new line\n");
        assert_eq!(next.start, c.end);
    }

    #[test]
    fn pages_backwards_on_line_boundaries() {
        // ~100 bytes a line: a few chunks' worth.
        let dir = tempfile::tempdir().unwrap();
        let (path, lines) = write_lines(dir.path(), 6000);
        let tail = read_lines(&path, 0, None).unwrap();
        assert!(tail.start > 0);
        assert_eq!(tail.end, tail.size);

        let mut text = tail.text;
        let mut start = tail.start;
        while start > 0 {
            let older = read_lines(&path, 0, Some(start)).unwrap();
            assert_eq!(older.end, start);
            text = older.text + &text;
            start = older.start;
        }
        assert_eq!(text.lines().collect::<Vec<_>>(), lines);
    }

    #[test]
    fn classifies_finished_logs() {
        let dir = tempfile::tempdir().unwrap();
        let t = "2026-09-26 14:30:05.123";
        let cases = [
            (
                format!(
                    "{t} [INFO] [bananium::lifecycle] {} window closed\n",
                    logging::SHUTDOWN_MARKER
                ),
                LogStatus::Closed,
            ),
            (
                format!(
                    "{t} [ERROR] [bananium::panic] {}boom (at a.rs:1:1, thread 'main')\n   0: frame\n",
                    logging::PANIC_MARKER
                ),
                LogStatus::Crashed,
            ),
            (
                format!("{t} [INFO] [bananium_api] launched\n"),
                LogStatus::Unclean,
            ),
        ];
        for (i, (text, status)) in cases.into_iter().enumerate() {
            let path = dir.path().join(format!("{i}.log"));
            std::fs::write(&path, text).unwrap();
            let log = describe(&path, None).unwrap();
            assert_eq!(log.status, status);
            assert!(log.reason.is_some());
        }
    }
}
