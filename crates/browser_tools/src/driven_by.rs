use serde::{Deserialize, Serialize};

/// Who is currently driving a browser session — a human via the GUI, the
/// agent via the tool, or nobody (idle). Surfaced so the GUI and AIUI can
/// hand a session off without stepping on each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrivenBy {
    #[default]
    Idle,
    Human,
    Agent,
}

impl DrivenBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}
