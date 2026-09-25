//! Reading the per-launch log files `Session::launch` redirects a game's
//! stdout/stderr into, so a frontend can tail them without touching the
//! filesystem itself.

use std::io::{Read, Seek, SeekFrom};
use std::time::UNIX_EPOCH;

use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, LogFile};

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
