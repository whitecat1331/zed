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
