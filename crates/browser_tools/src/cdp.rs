use anyhow::{Context, Result, anyhow};
use async_tungstenite::tungstenite::Message;
use async_tungstenite::{WebSocketStream, client_async};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use smol::net::TcpStream;
use url::Url;

/// An error returned by the Chrome DevTools Protocol client.
#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    #[error("CDP error {code}: {message}")]
    Protocol { code: i64, message: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Maximum number of buffered events kept in memory before older ones drop.
const MAX_BUFFERED_EVENTS: usize = 500;

/// A minimal CDP client over a single websocket connection.
///
/// Commands are sent serially; events received while awaiting a response are
/// buffered (bounded) and can be inspected with [`CdpClient::recent_events`]
/// or drained with [`CdpClient::take_events`]. An optional `session_id` scopes
/// commands to a flattened target session.
pub struct CdpClient {
    socket: WebSocketStream<TcpStream>,
    next_id: u64,
    session_id: Option<String>,
    events: Vec<Value>,
}

impl CdpClient {
    /// Connect to a CDP websocket endpoint (e.g. a Chromium page target).
    pub async fn connect(endpoint: &str) -> Result<Self> {
        let url = Url::parse(endpoint).context("invalid CDP websocket url")?;
        let host = url.host_str().context("CDP websocket url has no host")?;
        let port = url
            .port_or_known_default()
            .context("CDP websocket url has no port")?;
        let stream = TcpStream::connect((host, port)).await?;
        let (socket, _) = client_async(endpoint, stream).await?;
        Ok(Self {
            socket,
            next_id: 0,
            session_id: None,
            events: Vec::new(),
        })
    }

    /// Scope subsequent commands to a flattened target session.
    pub fn set_session_id(&mut self, session_id: Option<String>) {
        self.session_id = session_id;
    }

    /// Send a command and await its response, buffering any events in between.
    pub async fn send_command(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let Some(session_id) = &self.session_id {
            request["sessionId"] = json!(session_id);
        }
        self.socket
            .send(Message::Text(request.to_string().into()))
            .await
            .context("failed to send CDP command")?;

        loop {
            let message = self
                .socket
                .next()
                .await
                .context("CDP connection closed while awaiting response")??;
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Message::Close(_) => return Err(anyhow!("CDP connection closed")),
                _ => continue,
            };
            let value: Value = serde_json::from_str(&text).context("invalid CDP message")?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown CDP error")
                        .to_string();
                    return Err(CdpError::Protocol { code, message }.into());
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
            if value.get("method").is_some() {
                if self.events.len() >= MAX_BUFFERED_EVENTS {
                    self.events.remove(0);
                }
                self.events.push(value);
            }
        }
    }

    /// Borrow the most recent buffered events without draining them.
    pub fn recent_events(&self) -> &[Value] {
        &self.events
    }

    /// Drain buffered events (console, network, etc.) since the last drain.
    pub fn take_events(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.events)
    }
}
