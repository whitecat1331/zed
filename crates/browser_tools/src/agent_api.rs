use crate::cdp::CdpClient;
use crate::session::BrowserSession;
use anyhow::{Context, Result, anyhow};
use futures::{AsyncBufReadExt, StreamExt};
use serde_json::{Value, json};
use smol::process::Command;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The project-side browser automation surface, mirroring `AgentDebuggerApi`.
///
/// It is cheap to clone (shared session store) so an agent tool can hold one
/// instance across turns without recreating browser sessions.
#[derive(Clone)]
pub struct AgentBrowserApi {
    sessions: Arc<smol::lock::Mutex<HashMap<u64, BrowserSession>>>,
    next_session_id: Arc<AtomicU64>,
    chromium_path: Option<PathBuf>,
}

impl AgentBrowserApi {
    pub fn new(chromium_path: Option<PathBuf>) -> Self {
        Self {
            sessions: Arc::new(smol::lock::Mutex::new(HashMap::new())),
            next_session_id: Arc::new(AtomicU64::new(0)),
            chromium_path,
        }
    }

    pub async fn list_sessions(&self) -> Vec<Value> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|session| json!({ "session_id": session.id, "url": session.url }))
            .collect()
    }

    pub async fn start_session(&self, url: &str) -> Result<u64> {
        let chromium = self.chromium_binary()?;
        let profile_dir = std::env::temp_dir().join(format!("zed-browser-{}", std::process::id()));

        let mut child = Command::new(&chromium)
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("about:blank")
            .stdout(smol::process::Stdio::null())
            .stderr(smol::process::Stdio::piped())
            .spawn()
            .context("failed to launch Chromium")?;

        let stderr = child.stderr.take().context("Chromium has no stderr")?;
        let endpoint = smol::future::race(read_devtools_endpoint(stderr), async {
            smol::Timer::after(Duration::from_secs(15)).await;
            Err(anyhow!("timed out waiting for Chromium DevTools endpoint"))
        })
        .await?;

        let mut client = CdpClient::connect(&endpoint).await?;
        let create_result = client
            .send_command("Target.createTarget", json!({ "url": url }))
            .await?;
        let target_id = create_result
            .get("targetId")
            .and_then(Value::as_str)
            .context("Target.createTarget returned no targetId")?
            .to_string();

        let attach_result = client
            .send_command(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;
        let session_id = attach_result
            .get("sessionId")
            .and_then(Value::as_str)
            .context("Target.attachToTarget returned no sessionId")?
            .to_string();
        client.set_session_id(Some(session_id.clone()));

        client.send_command("Page.enable", json!({})).await?;
        client.send_command("Runtime.enable", json!({})).await?;
        client.send_command("Network.enable", json!({})).await?;

        let id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        self.sessions.lock().await.insert(
            id,
            BrowserSession {
                id,
                client,
                session_id,
                target_id,
                url: url.to_string(),
            },
        );
        Ok(id)
    }

    pub async fn navigate(&self, session_id: u64, url: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;
        session
            .client
            .send_command("Page.navigate", json!({ "url": url }))
            .await?;
        session.url = url.to_string();
        Ok(())
    }

    pub async fn snapshot(&self, session_id: u64) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;

        let document = session
            .client
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": "JSON.stringify({ url: location.href, title: document.title, text: document.body ? document.body.innerText.slice(0, 10000) : '' })",
                    "returnByValue": true,
                }),
            )
            .await?;
        let events = session.client.take_events();

        Ok(json!({
            "url": session.url,
            "document": document,
            "events": events,
        }))
    }

    pub async fn stop_session(&self, session_id: u64) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .await
            .remove(&session_id)
            .context("unknown browser session")?;
        // Best-effort target teardown; the browser process keeps running but the
        // target is closed.
        let mut client = session.client;
        client.set_session_id(None);
        client
            .send_command(
                "Target.closeTarget",
                json!({ "targetId": session.target_id }),
            )
            .await?;
        Ok(())
    }

    fn chromium_binary(&self) -> Result<PathBuf> {
        if let Some(path) = &self.chromium_path {
            return Ok(path.clone());
        }
        discover_chromium().context("no Chromium binary found; set --chromium-path")
    }
}

async fn read_devtools_endpoint(stderr: smol::process::ChildStderr) -> Result<String> {
    let mut lines = futures::io::BufReader::new(stderr).lines();
    while let Some(line) = lines.next().await {
        let line = line.context("failed to read Chromium stderr")?;
        if let Some(endpoint) = parse_devtools_endpoint(&line) {
            return Ok(endpoint);
        }
    }
    Err(anyhow!(
        "Chromium exited before exposing a DevTools endpoint"
    ))
}

fn parse_devtools_endpoint(line: &str) -> Option<String> {
    let marker = "DevTools listening on ";
    let start = line.find(marker)? + marker.len();
    Some(line[start..].trim().to_string())
}

fn discover_chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ZED_BROWSER_CHROMIUM_PATH") {
        return Some(PathBuf::from(path));
    }
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.exists() {
            return Some(path.to_path_buf());
        }
    }
    None
}
