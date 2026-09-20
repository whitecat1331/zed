use crate::cdp::CdpClient;
use crate::network::{NetworkControlState, NetworkStore};
use std::collections::HashMap;

/// A running browser session: a browser-level connection plus its targets.
pub struct BrowserSession {
    /// Opaque session id used by the agent tool.
    pub id: u64,
    /// The browser-level websocket connection.
    pub client: CdpClient,
    /// Flattened targets (tabs, workers, etc.) keyed by CDP target id.
    pub targets: HashMap<String, BrowserTarget>,
    /// The target operations default to when none is specified.
    pub active_target_id: Option<String>,
    /// Handle to the Chromium process, so teardown can close the browser.
    pub child: smol::process::Child,
    /// Typed request store fed from captured `Network.*` events.
    pub network_store: NetworkStore,
    /// Mutable network control state (throttle, offline, blocking, interception).
    pub control: NetworkControlState,
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
    pub url: String,
}
