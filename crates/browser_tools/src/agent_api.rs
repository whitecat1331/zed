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
            .send_command("Target.createTarget", json!({ "url": "about:blank" }))
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
        client
            .send_command("Page.navigate", json!({ "url": url }))
            .await?;
        wait_for_page_load(&mut client, url).await?;

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

        let page = evaluate_value(&mut session.client, SNAPSHOT_EXPRESSION).await?;
        let events = session.client.recent_events();

        Ok(json!({
            "url": page
                .get("url")
                .cloned()
                .unwrap_or_else(|| Value::String(session.url.clone())),
            "title": page.get("title").cloned().unwrap_or(Value::Null),
            "text": page.get("text").cloned().unwrap_or(Value::Null),
            "elements": page
                .get("elements")
                .cloned()
                .unwrap_or_else(|| Value::Array(vec![])),
            "console": filter_events(events, is_console_event, 100),
            "network": filter_events(events, is_network_event, 100),
        }))
    }

    pub async fn evaluate(&self, session_id: u64, expression: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;

        let result = session
            .client
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;
        let evaluated = result.get("result").cloned().unwrap_or(Value::Null);
        let exception = result
            .get("exceptionDetails")
            .and_then(|details| details.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok(json!({
            "value": evaluated.get("value").cloned().unwrap_or(Value::Null),
            "type": evaluated.get("type").cloned().unwrap_or(Value::Null),
            "description": evaluated.get("description").cloned().unwrap_or(Value::Null),
            "exception": exception,
        }))
    }

    pub async fn click(&self, session_id: u64, selector: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;

        let expression = format!(
            "(() => {{ const el = document.querySelector({selector}); if (!el) return {{ clicked: false, reason: 'no element matches selector' }}; el.click(); return {{ clicked: true, tag: el.tagName.toLowerCase(), text: (el.innerText || el.value || '').toString().slice(0, 500) }}; }})()",
            selector = serde_json::to_string(selector)?,
        );
        let value = evaluate_value(&mut session.client, &expression).await?;
        Ok(json!({ "clicked": value }))
    }

    pub async fn type_text(&self, session_id: u64, selector: &str, text: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;

        let focus = format!(
            "(() => {{ const el = document.querySelector({selector}); if (!el) return {{ focused: false, reason: 'no element matches selector' }}; el.focus(); return {{ focused: true, tag: el.tagName.toLowerCase() }}; }})()",
            selector = serde_json::to_string(selector)?,
        );
        let focused = evaluate_value(&mut session.client, &focus).await?;
        if focused.get("focused").and_then(Value::as_bool) != Some(true) {
            return Ok(json!({
                "typed": false,
                "reason": focused
                    .get("reason")
                    .cloned()
                    .unwrap_or_else(|| Value::String("no element matches selector".into())),
            }));
        }

        session
            .client
            .send_command("Input.insertText", json!({ "text": text }))
            .await?;
        Ok(json!({ "typed": true, "selector": selector, "text": text }))
    }

    pub async fn read_console(&self, session_id: u64) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;
        let events = session.client.recent_events();
        Ok(json!({ "console": filter_events(events, is_console_event, 100) }))
    }

    pub async fn read_network(&self, session_id: u64) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .context("unknown browser session")?;
        let events = session.client.recent_events();
        Ok(json!({ "network": filter_events(events, is_network_event, 100) }))
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

const SNAPSHOT_EXPRESSION: &str = r#"(function() {
  var body = document.body ? document.body.innerText.slice(0, 10000) : '';
  var elements = [];
  var all = document.querySelectorAll('a, button, input, textarea, select, [role="button"], [role="link"], [role="textbox"], summary');
  var seen = 0;
  for (var i = 0; i < all.length; i++) {
    if (seen >= 100) break;
    var el = all[i];
    var rect = el.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) continue;
    var label = (el.getAttribute('aria-label') || el.innerText || el.value || el.getAttribute('placeholder') || el.getAttribute('name') || '').toString().trim().slice(0, 200);
    elements.push({
      tag: el.tagName.toLowerCase(),
      id: el.id || null,
      type: el.getAttribute('type') || null,
      label: label,
      selector: cssPath(el),
    });
    seen += 1;
  }
  return { url: location.href, title: document.title, text: body, elements: elements };

  function cssPath(el) {
    if (el.id) return '#' + CSS.escape(el.id);
    var parts = [];
    var node = el;
    while (node && node.nodeType === 1 && node !== document.body) {
      var name = node.tagName.toLowerCase();
      var nth = 1;
      var sibling = node;
      while ((sibling = sibling.previousElementSibling)) {
        if (sibling.tagName.toLowerCase() === name) nth += 1;
      }
      parts.unshift(name + ':nth-of-type(' + nth + ')');
      node = node.parentElement;
    }
    return parts.join(' > ');
  }
})()"#;

async fn evaluate_value(client: &mut CdpClient, expression: &str) -> Result<Value> {
    let result = client
        .send_command(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
            }),
        )
        .await?;
    Ok(result
        .get("result")
        .and_then(|value| value.get("value"))
        .cloned()
        .unwrap_or(Value::Null))
}

async fn wait_for_page_load(client: &mut CdpClient, url: &str) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let state = evaluate_value(
            client,
            "(function() { return { href: location.href, ready: document.readyState }; })()",
        )
        .await;
        let loaded = state
            .as_ref()
            .map(|value| {
                value
                    .get("href")
                    .and_then(Value::as_str)
                    .map(|href| href != "about:blank")
                    .unwrap_or(false)
                    && value.get("ready").and_then(Value::as_str) == Some("complete")
            })
            .unwrap_or(false);
        if loaded {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!("timed out waiting for {url} to load"));
        }
        smol::Timer::after(Duration::from_millis(100)).await;
    }
}

fn is_console_event(event: &Value) -> bool {
    event
        .get("method")
        .and_then(Value::as_str)
        .map(|method| method == "Runtime.consoleAPICalled")
        .unwrap_or(false)
}

fn is_network_event(event: &Value) -> bool {
    event
        .get("method")
        .and_then(Value::as_str)
        .map(|method| method.starts_with("Network."))
        .unwrap_or(false)
}

fn filter_events(events: &[Value], predicate: fn(&Value) -> bool, max: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for event in events.iter().rev() {
        if predicate(event) {
            out.push(event.clone());
            if out.len() >= max {
                break;
            }
        }
    }
    out.reverse();
    out
}
