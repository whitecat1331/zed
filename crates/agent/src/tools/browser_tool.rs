use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result};
use browser_tools::AgentBrowserApi;
use gpui::{App, SharedString, Task, WeakEntity};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{
    AgentTool, Thread, ToolCallEventStream, ToolInput, ToolPermissionContext,
};

/// Interact with a browser the agent controls. Read-only operations such as
/// `list_sessions`, `snapshot`, `read_console`, and `read_network` are available
/// in Ask mode. Operations that start sessions, navigate, click, type, evaluate
/// JavaScript, or stop sessions require Write mode and user permission.
///
/// The observation surface is text-first: `snapshot` returns the page URL,
/// title, body text, and recent console/network events. Screenshots are not a
/// requirement; text-only models get full interactive control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserOperation {
    /// List active browser sessions.
    #[default]
    ListSessions,
    /// Launch a browser (Chromium over CDP) with a target URL.
    StartSession,
    /// Navigate a browser session to a URL.
    Navigate,
    /// Capture a bounded text snapshot of the current page.
    Snapshot,
    /// Click an element in the current page.
    Click,
    /// Type text into the current page.
    Type,
    /// Evaluate a JavaScript expression in the current page.
    Evaluate,
    /// Read recent console events.
    ReadConsole,
    /// Read recent network events.
    ReadNetwork,
    /// Stop a browser session.
    StopSession,
}

/// A single browser operation and the fields it needs.
///
/// Kept as a flat struct (rather than a tagged enum) so the JSON schema
/// advertised to language-model providers is a `type: "object"` with an
/// `operation` discriminator.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BrowserToolInput {
    /// Which browser operation to run.
    pub operation: BrowserOperation,
    /// Browser session id, used by most operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u64>,
    /// URL, used by `start_session` and `navigate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Element selector or ref, used by `click` and `type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Text to type, used by `type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// JavaScript expression to evaluate, used by `evaluate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Keep the browser process running after stopping the session. Defaults to
    /// false, which closes the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_open: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BrowserToolOutput {
    Success {
        operation: String,
        message: String,
        data: Value,
    },
    Error {
        operation: Option<String>,
        error: String,
    },
}

impl From<BrowserToolOutput> for LanguageModelToolResultContent {
    fn from(output: BrowserToolOutput) -> Self {
        match &output {
            BrowserToolOutput::Success {
                operation,
                message,
                data,
            } => {
                let data = serde_json::to_string_pretty(data).unwrap_or_else(|error| {
                    format!("<failed to serialize browser output: {error}>")
                });
                format!("Browser `{operation}` succeeded: {message}\n\n```json\n{data}\n```")
                    .into()
            }
            BrowserToolOutput::Error { operation, error } => {
                let operation = operation.as_deref().unwrap_or("unknown");
                format!("Browser `{operation}` failed: {error}").into()
            }
        }
    }
}

pub struct BrowserTool {
    api: AgentBrowserApi,
    thread: WeakEntity<Thread>,
}

impl BrowserTool {
    pub fn new(thread: WeakEntity<Thread>) -> Self {
        Self {
            api: AgentBrowserApi::new(None),
            thread,
        }
    }

    fn is_ask_profile(&self, cx: &App) -> bool {
        self.thread
            .read_with(cx, |thread, _| thread.profile().as_str() == "ask")
            .unwrap_or(false)
    }

    fn ensure_write_mode(&self, operation: &str, cx: &gpui::AsyncApp) -> Result<()> {
        if cx.update(|cx| self.is_ask_profile(cx)) {
            anyhow::bail!(
                "browser.{operation} is not available in Ask mode. Switch to Write mode to start sessions, navigate, or interact with the browser."
            );
        }
        Ok(())
    }

    async fn run_operation(
        self: Arc<Self>,
        input: BrowserToolInput,
        operation: String,
        event_stream: ToolCallEventStream,
        cx: &mut gpui::AsyncApp,
    ) -> Result<BrowserToolOutput> {
        match input.operation {
            BrowserOperation::ListSessions => {
                let sessions = self.api.list_sessions().await;
                Ok(success(operation, "listed browser sessions", Value::Array(sessions)))
            }
            BrowserOperation::Snapshot => {
                let session_id = input
                    .session_id
                    .context("session_id is required for browser snapshot")?;
                let snapshot = self.api.snapshot(session_id).await?;
                Ok(success(operation, "captured browser snapshot", snapshot))
            }
            BrowserOperation::StartSession => {
                self.ensure_write_mode(&operation, cx)?;
                let url = input
                    .url
                    .context("url is required for browser start_session")?;
                authorize_browser_operation(
                    &event_stream,
                    "Start browser session",
                    permission_inputs(&operation, [format!("url:{url}")]),
                    cx,
                )
                .await?;
                let session_id = self.api.start_session(&url).await?;
                Ok(success(
                    operation,
                    "started browser session",
                    json!({ "session_id": session_id }),
                ))
            }
            BrowserOperation::Navigate => {
                self.ensure_write_mode(&operation, cx)?;
                let session_id = input
                    .session_id
                    .context("session_id is required for browser navigate")?;
                let url = input.url.context("url is required for browser navigate")?;
                authorize_browser_operation(
                    &event_stream,
                    "Navigate browser",
                    permission_inputs(&operation, [format!("session_id:{session_id} url:{url}")]),
                    cx,
                )
                .await?;
                self.api.navigate(session_id, &url).await?;
                Ok(success(operation, "navigated", json!({})))
            }
            BrowserOperation::StopSession => {
                self.ensure_write_mode(&operation, cx)?;
                let session_id = input
                    .session_id
                    .context("session_id is required for browser stop_session")?;
                let keep_open = input.keep_open.unwrap_or(false);
                authorize_browser_operation(
                    &event_stream,
                    "Stop browser session",
                    permission_inputs(&operation, [format!("session_id:{session_id}")]),
                    cx,
                )
                .await?;
                self.api.stop_session(session_id, keep_open).await?;
                Ok(success(operation, "stopped browser session", json!({})))
            }
            BrowserOperation::Click => {
                self.ensure_write_mode(&operation, cx)?;
                let session_id = input
                    .session_id
                    .context("session_id is required for browser click")?;
                let selector = input
                    .selector
                    .context("selector is required for browser click")?;
                authorize_browser_operation(
                    &event_stream,
                    "Click browser element",
                    permission_inputs(&operation, [format!("session_id:{session_id} selector:{selector}")]),
                    cx,
                )
                .await?;
                let result = self.api.click(session_id, &selector).await?;
                Ok(success(operation, "clicked element", result))
            }
            BrowserOperation::Type => {
                self.ensure_write_mode(&operation, cx)?;
                let session_id = input
                    .session_id
                    .context("session_id is required for browser type")?;
                let selector = input
                    .selector
                    .context("selector is required for browser type")?;
                let text = input
                    .text
                    .context("text is required for browser type")?;
                authorize_browser_operation(
                    &event_stream,
                    "Type into browser",
                    permission_inputs(
                        &operation,
                        [format!("session_id:{session_id} selector:{selector} text:{text}")],
                    ),
                    cx,
                )
                .await?;
                let result = self.api.type_text(session_id, &selector, &text).await?;
                Ok(success(operation, "typed text", result))
            }
            BrowserOperation::Evaluate => {
                self.ensure_write_mode(&operation, cx)?;
                let session_id = input
                    .session_id
                    .context("session_id is required for browser evaluate")?;
                let expression = input
                    .expression
                    .context("expression is required for browser evaluate")?;
                authorize_browser_operation(
                    &event_stream,
                    "Evaluate browser JavaScript",
                    permission_inputs(&operation, [format!("session_id:{session_id} expression:{expression}")]),
                    cx,
                )
                .await?;
                let result = self.api.evaluate(session_id, &expression).await?;
                Ok(success(operation, "evaluated expression", result))
            }
            BrowserOperation::ReadConsole => {
                let session_id = input
                    .session_id
                    .context("session_id is required for browser read_console")?;
                let result = self.api.read_console(session_id).await?;
                Ok(success(operation, "read console events", result))
            }
            BrowserOperation::ReadNetwork => {
                let session_id = input
                    .session_id
                    .context("session_id is required for browser read_network")?;
                let result = self.api.read_network(session_id).await?;
                Ok(success(operation, "read network events", result))
            }
        }
    }
}

impl AgentTool for BrowserTool {
    type Input = BrowserToolInput;
    type Output = BrowserToolOutput;

    const NAME: &'static str = "browser";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("Browser: {}", operation_name(&input)).into(),
            Err(value) => value
                .get("operation")
                .and_then(|value| value.as_str())
                .map(|operation| format!("Browser: {operation}").into())
                .unwrap_or_else(|| "Browser".into()),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|error| BrowserToolOutput::Error {
                    operation: None,
                    error: format!("Failed to receive browser tool input: {error}"),
                })?;
            let operation = operation_name(&input).to_string();
            match self
                .run_operation(input, operation.clone(), event_stream, cx)
                .await
            {
                Ok(output) => Ok(output),
                Err(error) => Err(BrowserToolOutput::Error {
                    operation: Some(operation),
                    error: error.to_string(),
                }),
            }
        })
    }
}

async fn authorize_browser_operation(
    event_stream: &ToolCallEventStream,
    title: impl Into<String>,
    input_values: Vec<String>,
    cx: &mut gpui::AsyncApp,
) -> Result<()> {
    let title = title.into();
    let task = cx.update(|cx| {
        event_stream.authorize(
            title,
            ToolPermissionContext::new(BrowserTool::NAME, input_values),
            cx,
        )
    });
    task.await
}

fn permission_inputs(operation: &str, values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut inputs = values.into_iter().collect::<Vec<_>>();
    if inputs.is_empty() {
        inputs.push(operation.to_string());
    } else {
        for input in &mut inputs {
            *input = format!("{operation} {input}");
        }
    }
    inputs
}

fn success(operation: String, message: impl Into<String>, data: Value) -> BrowserToolOutput {
    BrowserToolOutput::Success {
        operation,
        message: message.into(),
        data,
    }
}

fn operation_name(input: &BrowserToolInput) -> &'static str {
    match input.operation {
        BrowserOperation::ListSessions => "list_sessions",
        BrowserOperation::StartSession => "start_session",
        BrowserOperation::Navigate => "navigate",
        BrowserOperation::Snapshot => "snapshot",
        BrowserOperation::Click => "click",
        BrowserOperation::Type => "type",
        BrowserOperation::Evaluate => "evaluate",
        BrowserOperation::ReadConsole => "read_console",
        BrowserOperation::ReadNetwork => "read_network",
        BrowserOperation::StopSession => "stop_session",
    }
}
