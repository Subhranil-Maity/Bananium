//! Discord's IPC wire format and socket discovery.
//!
//! Every message is a frame: a little-endian `u32` opcode, a little-endian
//! `u32` payload length, then that many bytes of JSON.

use std::path::PathBuf;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// Frame opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opcode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl TryFrom<u32> for Opcode {
    type Error = Error;

    fn try_from(op: u32) -> Result<Self> {
        Ok(match op {
            0 => Opcode::Handshake,
            1 => Opcode::Frame,
            2 => Opcode::Close,
            3 => Opcode::Ping,
            4 => Opcode::Pong,
            other => return Err(Error::Protocol(format!("unknown opcode {other}"))),
        })
    }
}

/// Real replies are a few KiB at most; anything bigger means we've lost
/// frame alignment (or aren't talking to Discord), and allocating the
/// claimed length would be a bad idea.
const MAX_FRAME_LEN: u32 = 64 * 1024;

/// Anything a frame can travel over: a named pipe, a Unix socket, or an
/// in-memory duplex in tests.
pub(crate) trait IpcStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> IpcStream for T {}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    op: Opcode,
    payload: &Value,
) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&(op as u32).to_le_bytes());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<(Opcode, Value)> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header).await?;
    let op = Opcode::try_from(u32::from_le_bytes(header[..4].try_into().unwrap()))?;
    let len = u32::from_le_bytes(header[4..].try_into().unwrap());
    if len > MAX_FRAME_LEN {
        return Err(Error::Protocol(format!(
            "frame of {len} bytes is too large"
        )));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body)?
    };
    Ok((op, value))
}

/// Every place a Discord client may be listening, in the order the
/// official SDK tries them: slots `0..10`, the first live one wins.
pub(crate) fn candidate_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        (0..10)
            .map(|i| PathBuf::from(format!(r"\\?\pipe\discord-ipc-{i}")))
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut dirs = Vec::new();
        for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
            if let Some(v) = std::env::var_os(var) {
                dirs.push(PathBuf::from(v));
            }
        }
        dirs.push(PathBuf::from("/tmp"));
        dirs.dedup();
        let mut paths = Vec::new();
        for i in 0..10 {
            for dir in &dirs {
                // Plain install, then the Flatpak and Snap sandboxes, which
                // put the socket in their own subdirectory.
                for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
                    paths.push(dir.join(sub).join(format!("discord-ipc-{i}")));
                }
            }
        }
        paths
    }
}

/// Connect to the first Discord IPC socket that accepts, or
/// [`Error::NotRunning`] when none does.
pub(crate) async fn connect() -> Result<Box<dyn IpcStream>> {
    for path in candidate_paths() {
        match open(&path).await {
            Ok(stream) => {
                tracing::debug!(path = %path.display(), "connected to Discord IPC");
                return Ok(stream);
            }
            Err(e) => tracing::trace!(path = %path.display(), error = %e, "no Discord IPC here"),
        }
    }
    Err(Error::NotRunning)
}

#[cfg(windows)]
async fn open(path: &std::path::Path) -> std::io::Result<Box<dyn IpcStream>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    Ok(Box::new(ClientOptions::new().open(path)?))
}

#[cfg(not(windows))]
async fn open(path: &std::path::Path) -> std::io::Result<Box<dyn IpcStream>> {
    Ok(Box::new(tokio::net::UnixStream::connect(path).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn frame_round_trips_with_little_endian_header() {
        let mut buf = Vec::new();
        write_frame(&mut buf, Opcode::Frame, &json!({"a": 1}))
            .await
            .unwrap();
        assert_eq!(&buf[..4], &1u32.to_le_bytes());
        assert_eq!(&buf[4..8], &7u32.to_le_bytes());
        assert_eq!(&buf[8..], br#"{"a":1}"#);

        let (op, value) = read_frame(&mut buf.as_slice()).await.unwrap();
        assert_eq!(op, Opcode::Frame);
        assert_eq!(value, json!({"a": 1}));
    }

    #[tokio::test]
    async fn rejects_unknown_opcode_and_oversized_frames() {
        let mut bad_op = 9u32.to_le_bytes().to_vec();
        bad_op.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            read_frame(&mut bad_op.as_slice()).await,
            Err(Error::Protocol(_))
        ));

        let mut huge = 1u32.to_le_bytes().to_vec();
        huge.extend_from_slice(&(MAX_FRAME_LEN + 1).to_le_bytes());
        assert!(matches!(
            read_frame(&mut huge.as_slice()).await,
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn candidates_cover_all_ten_slots() {
        let paths = candidate_paths();
        for i in 0..10 {
            let name = format!("discord-ipc-{i}");
            assert!(paths.iter().any(|p| p.to_string_lossy().ends_with(&name)));
        }
    }
}
