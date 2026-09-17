use std::path::PathBuf;
use std::sync::Arc;

use crate::{AgentTool, MemoryStore, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, AppContext as _, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The operation to perform on the agent's persistent memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperation {
    /// Store a fact under `key`, overwriting any previous value.
    Remember,
    /// Read the fact stored under `key`.
    #[default]
    Recall,
    /// Remove the fact stored under `key`.
    Forget,
}

/// Read or update persistent agent memory.
///
/// Memory is a flat `key → value` store that persists across threads and runs.
/// Use it to remember facts you'll need later — URLs, hostnames, decisions,
/// preferences — and recall them on demand. Only the keys are shown in your
/// system prompt; read a value with `operation: "recall"` when you actually
/// need it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolInput {
    /// Which memory operation to perform.
    pub operation: MemoryOperation,
    /// The memory key. Short, lowercase, hyphenated identifiers work best.
    pub key: String,
    /// The value to store. Required for `operation: "remember"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MemoryToolOutput {
    Success { message: String },
    Error { error: String },
}

impl From<MemoryToolOutput> for LanguageModelToolResultContent {
    fn from(value: MemoryToolOutput) -> Self {
        match value {
            MemoryToolOutput::Success { message } => message.into(),
            MemoryToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct MemoryTool;

impl AgentTool for MemoryTool {
    type Input = MemoryToolInput;
    type Output = MemoryToolOutput;

    const NAME: &'static str = "memory";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => match input.operation {
                MemoryOperation::Remember => format!("Remembering {}", input.key).into(),
                MemoryOperation::Recall => format!("Recalling {}", input.key).into(),
                MemoryOperation::Forget => format!("Forgetting {}", input.key).into(),
            },
            Err(_) => "Updating memory".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.background_spawn(async move {
            let input = input
                .recv()
                .await
                .map_err(|error| MemoryToolOutput::Error {
                    error: format!("Failed to receive memory tool input: {error}"),
                })?;
            run_operation(input, MemoryStore::default_path())
        })
    }
}

/// Performs a memory operation against the on-disk store at `path`. The store
/// is a single tiny JSON file, so it is read and written synchronously on the
/// background thread.
fn run_operation(
    input: MemoryToolInput,
    path: PathBuf,
) -> Result<MemoryToolOutput, MemoryToolOutput> {
    let mut store = MemoryStore::load(path).map_err(|error| MemoryToolOutput::Error {
        error: format!("Failed to load memory: {error}"),
    })?;

    match input.operation {
        MemoryOperation::Remember => {
            let value = input.value.ok_or_else(|| MemoryToolOutput::Error {
                error: "`value` is required for `operation: \"remember\"`.".to_string(),
            })?;
            store
                .remember(&input.key, &value)
                .map_err(|error| MemoryToolOutput::Error {
                    error: error.to_string(),
                })?;
            Ok(MemoryToolOutput::Success {
                message: format!("Remembered {} = {value}", input.key),
            })
        }
        MemoryOperation::Recall => match store.recall(&input.key) {
            Some(value) => Ok(MemoryToolOutput::Success {
                message: value.to_string(),
            }),
            None => Ok(MemoryToolOutput::Success {
                message: format!("No memory stored under key \"{}\".", input.key),
            }),
        },
        MemoryOperation::Forget => {
            let removed = store
                .forget(&input.key)
                .map_err(|error| MemoryToolOutput::Error {
                    error: error.to_string(),
                })?;
            if removed {
                Ok(MemoryToolOutput::Success {
                    message: format!("Forgot \"{}\".", input.key),
                })
            } else {
                Ok(MemoryToolOutput::Success {
                    message: format!("No memory stored under key \"{}\".", input.key),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remember(path: PathBuf, key: &str, value: &str) -> MemoryToolOutput {
        run_operation(
            MemoryToolInput {
                operation: MemoryOperation::Remember,
                key: key.to_string(),
                value: Some(value.to_string()),
            },
            path,
        )
        .unwrap()
    }

    #[test]
    fn remember_then_recall_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.json");

        remember(path.clone(), "host", "t-rex.proxmox.local");

        let recalled = run_operation(
            MemoryToolInput {
                operation: MemoryOperation::Recall,
                key: "host".to_string(),
                value: None,
            },
            path,
        )
        .unwrap();
        assert!(matches!(
            recalled,
            MemoryToolOutput::Success { message } if message == "t-rex.proxmox.local"
        ));
    }

    #[test]
    fn remember_requires_value() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_operation(
            MemoryToolInput {
                operation: MemoryOperation::Remember,
                key: "key".to_string(),
                value: None,
            },
            dir.path().join("memory.json"),
        );
        assert!(matches!(
            result,
            Err(MemoryToolOutput::Error { error }) if error.contains("`value` is required")
        ));
    }

    #[test]
    fn recall_missing_key_is_a_success() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_operation(
            MemoryToolInput {
                operation: MemoryOperation::Recall,
                key: "missing".to_string(),
                value: None,
            },
            dir.path().join("memory.json"),
        )
        .unwrap();
        assert!(matches!(
            result,
            MemoryToolOutput::Success { message } if message.contains("No memory stored")
        ));
    }

    #[test]
    fn forget_removes_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.json");

        remember(path.clone(), "drop", "value");
        let forgotten = run_operation(
            MemoryToolInput {
                operation: MemoryOperation::Forget,
                key: "drop".to_string(),
                value: None,
            },
            path.clone(),
        )
        .unwrap();
        assert!(matches!(
            forgotten,
            MemoryToolOutput::Success { message } if message.contains("Forgot")
        ));

        let reloaded = MemoryStore::load(path).unwrap();
        assert_eq!(reloaded.recall("drop"), None);
    }
}
