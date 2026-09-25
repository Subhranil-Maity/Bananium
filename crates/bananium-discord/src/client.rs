use std::time::Duration;

use serde_json::{json, Value};

use crate::activity::Activity;
use crate::error::{Error, Result};
use crate::ipc::{self, IpcStream, Opcode};

/// How long Discord gets to answer a handshake or a command. It replies in
/// milliseconds when healthy; a silent socket means something is wrong.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// A live, handshaken connection to the local Discord client, bound to one
/// Discord application (`client_id`). The app's name is what Discord shows
/// as `Playing <name>`.
///
/// Dropping it closes the socket, and Discord clears the presence on its
/// own when that happens.
pub struct DiscordClient {
    stream: Box<dyn IpcStream>,
    client_id: String,
    next_nonce: u64,
}

impl DiscordClient {
    /// Find Discord's IPC socket and handshake as `client_id`, once.
    pub async fn connect(client_id: &str) -> Result<Self> {
        let stream = ipc::connect().await?;
        Self::handshake(stream, client_id).await
    }

    /// Handshake over an already-open stream. Split out of `connect` so the
    /// protocol can be tested against an in-memory peer.
    pub(crate) async fn handshake(mut stream: Box<dyn IpcStream>, client_id: &str) -> Result<Self> {
        ipc::write_frame(
            &mut stream,
            Opcode::Handshake,
            &json!({ "v": 1, "client_id": client_id }),
        )
        .await?;
        let mut client = Self {
            stream,
            client_id: client_id.to_string(),
            next_nonce: 1,
        };
        tokio::time::timeout(REPLY_TIMEOUT, client.await_ready())
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(client)
    }

    /// The Discord application this connection presents as.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Show `activity` (sanitized first so one bad field can't get the whole
    /// update rejected), or clear the presence with `None`.
    pub async fn set_activity(&mut self, activity: Option<&Activity>) -> Result<()> {
        let activity = match activity {
            Some(a) => serde_json::to_value(a.sanitize())?,
            None => Value::Null,
        };
        let nonce = self.next_nonce.to_string();
        self.next_nonce += 1;
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": nonce,
        });
        ipc::write_frame(&mut self.stream, Opcode::Frame, &payload).await?;
        tokio::time::timeout(REPLY_TIMEOUT, self.await_reply(&nonce))
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Remove the presence but keep the connection open.
    pub async fn clear(&mut self) -> Result<()> {
        self.set_activity(None).await
    }

    /// Say goodbye properly. Dropping the client works too; this just lets
    /// Discord clear the presence immediately instead of on socket EOF.
    pub async fn close(mut self) -> Result<()> {
        ipc::write_frame(&mut self.stream, Opcode::Close, &json!({})).await
    }

    /// Read until Discord's `READY` dispatch, answering pings on the way.
    async fn await_ready(&mut self) -> Result<()> {
        loop {
            let frame = self.next_frame().await?;
            if frame["cmd"] == "DISPATCH" && frame["evt"] == "READY" {
                return Ok(());
            }
        }
    }

    /// Read until the reply carrying `nonce`, answering pings on the way.
    async fn await_reply(&mut self, nonce: &str) -> Result<()> {
        loop {
            let frame = self.next_frame().await?;
            if frame["nonce"] != nonce {
                continue;
            }
            if frame["evt"] == "ERROR" {
                return Err(Error::Rejected {
                    code: frame["data"]["code"].as_i64().unwrap_or_default(),
                    message: frame["data"]["message"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_string(),
                });
            }
            return Ok(());
        }
    }

    /// The next `FRAME` payload. Pings are answered and skipped; a `CLOSE`
    /// becomes [`Error::Closed`].
    async fn next_frame(&mut self) -> Result<Value> {
        loop {
            let (op, value) = ipc::read_frame(&mut self.stream).await?;
            match op {
                Opcode::Frame => return Ok(value),
                Opcode::Ping => ipc::write_frame(&mut self.stream, Opcode::Pong, &value).await?,
                Opcode::Close => {
                    return Err(Error::Closed {
                        code: value["code"].as_i64().unwrap_or_default(),
                        message: value["message"].as_str().unwrap_or("closed").to_string(),
                    })
                }
                Opcode::Handshake | Opcode::Pong => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    /// The Discord side of an in-memory connection.
    struct FakeDiscord(DuplexStream);

    impl FakeDiscord {
        async fn recv(&mut self) -> (Opcode, Value) {
            ipc::read_frame(&mut self.0).await.unwrap()
        }
        async fn send(&mut self, op: Opcode, v: Value) {
            ipc::write_frame(&mut self.0, op, &v).await.unwrap();
        }
        async fn ready(&mut self) {
            self.send(
                Opcode::Frame,
                json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}}),
            )
            .await;
        }
    }

    fn pair() -> (Box<dyn IpcStream>, FakeDiscord) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        (Box::new(a), FakeDiscord(b))
    }

    #[tokio::test]
    async fn handshake_then_set_activity_with_ping_in_between() {
        let (stream, mut discord) = pair();
        let server = tokio::spawn(async move {
            let (op, hello) = discord.recv().await;
            assert_eq!(op, Opcode::Handshake);
            assert_eq!(hello, json!({"v": 1, "client_id": "123"}));
            discord.send(Opcode::Ping, json!({"n": 1})).await;
            discord.ready().await;
            assert_eq!(discord.recv().await, (Opcode::Pong, json!({"n": 1})));

            let (op, cmd) = discord.recv().await;
            assert_eq!(op, Opcode::Frame);
            assert_eq!(cmd["cmd"], "SET_ACTIVITY");
            assert_eq!(cmd["args"]["activity"]["details"], "In the launcher");
            assert!(cmd["args"]["pid"].is_u64());
            // An unrelated event first: the client must keep waiting for
            // the reply with its own nonce.
            discord
                .send(
                    Opcode::Frame,
                    json!({"evt": "ACTIVITY_JOIN", "nonce": null}),
                )
                .await;
            discord
                .send(
                    Opcode::Frame,
                    json!({"cmd": "SET_ACTIVITY", "nonce": cmd["nonce"], "data": {}}),
                )
                .await;

            let (_, clear) = discord.recv().await;
            assert!(clear["args"]["activity"].is_null());
            discord
                .send(
                    Opcode::Frame,
                    json!({"cmd": "SET_ACTIVITY", "nonce": clear["nonce"]}),
                )
                .await;
        });

        let mut client = DiscordClient::handshake(stream, "123").await.unwrap();
        assert_eq!(client.client_id(), "123");
        client
            .set_activity(Some(&Activity::new().details("In the launcher")))
            .await
            .unwrap();
        client.clear().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn error_reply_is_rejected() {
        let (stream, mut discord) = pair();
        let server = tokio::spawn(async move {
            discord.recv().await;
            discord.ready().await;
            let (_, cmd) = discord.recv().await;
            discord
                .send(
                    Opcode::Frame,
                    json!({"evt": "ERROR", "nonce": cmd["nonce"],
                           "data": {"code": 4000, "message": "child \"state\" fails"}}),
                )
                .await;
        });
        let mut client = DiscordClient::handshake(stream, "1").await.unwrap();
        let err = client
            .set_activity(Some(&Activity::new().state("hi")))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Rejected { code: 4000, .. }), "{err:?}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn close_during_handshake_is_reported() {
        let (stream, mut discord) = pair();
        let server = tokio::spawn(async move {
            discord.recv().await;
            discord
                .send(
                    Opcode::Close,
                    json!({"code": 4000, "message": "Invalid Client ID"}),
                )
                .await;
        });
        let err = DiscordClient::handshake(stream, "bogus")
            .await
            .err()
            .unwrap();
        assert!(
            matches!(&err, Error::Closed { code: 4000, message } if message == "Invalid Client ID"),
            "{err:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn silent_peer_times_out() {
        let (stream, discord) = pair();
        let err = DiscordClient::handshake(stream, "1").await.err().unwrap();
        assert!(matches!(err, Error::Timeout), "{err:?}");
        drop(discord);
    }
}
