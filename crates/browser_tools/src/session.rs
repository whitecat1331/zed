use crate::cdp::CdpClient;

/// A running browser session: one page target flattened onto a browser websocket.
pub struct BrowserSession {
    /// Opaque session id used by the agent tool.
    pub id: u64,
    /// The browser-level websocket connection this session is flattened onto.
    pub client: CdpClient,
    /// Flattened CDP session id for the page target.
    pub session_id: String,
    /// CDP target id, used to close the target on teardown.
    pub target_id: String,
    /// Most recently navigated URL (best-effort).
    pub url: String,
    /// Handle to the Chromium process, so teardown can close the browser.
    pub child: smol::process::Child,
}
/// A single flattened target (page, worker, service worker, ...).
pub struct BrowserTarget {
    /// CDP target id, used to close/attach the target.
    pub target_id: String,
    /// Flattened CDP session id, used to scope commands to this target.
    pub session_id: String,
    /// CDP target type (`page`, `worker`, `service_worker`, ...).
    pub target_type: String,
    /// Most recently known URL (best-effort).
    pub url: String,
}
