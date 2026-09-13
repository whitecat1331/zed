use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use db::{
    sqlez::{
        bindable::{Bind, Column, StaticColumnCount},
        statement::Statement,
    },
    sqlez_macros::sql,
};
use gpui::App;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{WorkspaceDb, path_list::PathList};

/// A stable, minted identifier for a managed workspace.
///
/// Unlike the legacy [`crate::WorkspaceId`] (an auto-increment `i64` keyed by a
/// path set), this id is minted once on `create` and never changes, even as
/// projects are added to or removed from the workspace. It is stored as its
/// hyphenated string form, so it round-trips as a readable `TEXT` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManagedWorkspaceId(Uuid);

impl ManagedWorkspaceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Stable, hyphenated string form suitable for use as a key.
    pub fn to_key_string(&self) -> String {
        self.0.hyphenated().to_string()
    }

    pub fn from_key_string(key: &str) -> Result<Self> {
        Ok(Self(Uuid::parse_str(key)?))
    }
}

impl StaticColumnCount for ManagedWorkspaceId {}

impl Bind for ManagedWorkspaceId {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        self.to_key_string().bind(statement, start_index)
    }
}

impl Column for ManagedWorkspaceId {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (key, next_index) = String::column(statement, start_index)?;
        Ok((Self::from_key_string(&key)?, next_index))
    }
}

/// A managed workspace's identity row (name + timestamps), keyed by
/// [`ManagedWorkspaceId`].
#[derive(Debug, Clone)]
pub struct ManagedWorkspace {
    pub workspace_id: ManagedWorkspaceId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One project (root folder) belonging to a managed workspace.
#[derive(Debug, Clone)]
pub struct ManagedWorkspaceProject {
    pub workspace_id: ManagedWorkspaceId,
    pub path: PathBuf,
    pub position: i64,
    pub remote_connection_id: Option<i64>,
}

/// The single source of truth for workspace identity: a thin wrapper over
/// [`WorkspaceDb`] that owns the `managed_workspaces` /
/// `managed_workspace_projects` tables (distinct from the legacy path-keyed
/// `workspaces` layout table).
pub struct WorkspaceManager(WorkspaceDb);

impl std::ops::Deref for WorkspaceManager {
    type Target = WorkspaceDb;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl WorkspaceManager {
    pub fn new(db: WorkspaceDb) -> Self {
        Self(db)
    }

    pub fn global(cx: &App) -> Self {
        Self(WorkspaceDb::global(cx))
    }

    /// Comma-joined last path components — the default display name.
    pub fn derive_name(project_paths: &[PathBuf]) -> String {
        project_paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Mint a new managed workspace id and record its membership.
    pub fn create(&self, name: String, project_paths: Vec<PathBuf>) -> Result<ManagedWorkspaceId> {
        let workspace_id = ManagedWorkspaceId::new();
        let now = Utc::now().to_rfc3339();
        let key = workspace_id.to_key_string();

        self.exec_bound::<(&str, &str, &str, &str)>(sql! {
            INSERT INTO managed_workspaces (workspace_id, name, created_at, updated_at)
            VALUES (?, ?, ?, ?)
        })?((key.as_str(), name.as_str(), now.as_str(), now.as_str()))?;

        for (position, path) in project_paths.into_iter().enumerate() {
            let path = path.to_string_lossy().into_owned();
            self.exec_bound::<(&str, &str, i64)>(sql! {
                INSERT INTO managed_workspace_projects (workspace_id, path, position)
                VALUES (?, ?, ?)
            })?((key.as_str(), path.as_str(), position as i64))?;
        }

        Ok(workspace_id)
    }

    pub fn get(&self, workspace_id: ManagedWorkspaceId) -> Result<Option<ManagedWorkspace>> {
        let key = workspace_id.to_key_string();
        let row = self.select_row_bound::<&str, (String, String, String)>(sql! {
            SELECT name, created_at, updated_at
            FROM managed_workspaces
            WHERE workspace_id = ?
        })?(key.as_str())?;

        Ok(row.map(|(name, created_at, updated_at)| ManagedWorkspace {
            workspace_id,
            name,
            created_at: parse_timestamp(&created_at),
            updated_at: parse_timestamp(&updated_at),
        }))
    }

    pub fn projects(
        &self,
        workspace_id: ManagedWorkspaceId,
    ) -> Result<Vec<ManagedWorkspaceProject>> {
        let key = workspace_id.to_key_string();
        let rows = self.select_bound::<&str, (String, i64, Option<i64>)>(sql! {
            SELECT path, position, remote_connection_id
            FROM managed_workspace_projects
            WHERE workspace_id = ?
            ORDER BY position
        })?(key.as_str())?;

        Ok(rows
            .into_iter()
            .map(
                |(path, position, remote_connection_id)| ManagedWorkspaceProject {
                    workspace_id,
                    path: PathBuf::from(path),
                    position,
                    remote_connection_id,
                },
            )
            .collect())
    }

    /// Resolve the managed workspace whose project membership is exactly the
    /// given folder set, if any. This is the look-up used to tag a thread with
    /// its workspace id and to keep backfill idempotent.
    pub fn workspace_id_for_paths(&self, paths: &PathList) -> Result<Option<ManagedWorkspaceId>> {
        if paths.is_empty() {
            return Ok(None);
        }

        let mut target = paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        target.sort();

        let rows = self
            .select_bound::<(), (String, String)>(sql! {
                SELECT workspace_id, path FROM managed_workspace_projects ORDER BY workspace_id, position
            })?
            (())?;

        let mut by_workspace: collections::HashMap<String, Vec<String>> =
            collections::HashMap::default();
        for (workspace_id, path) in rows {
            by_workspace.entry(workspace_id).or_default().push(path);
        }

        for (workspace_id, mut project_paths) in by_workspace {
            project_paths.sort();
            if project_paths == target {
                return Ok(Some(ManagedWorkspaceId::from_key_string(&workspace_id)?));
            }
        }

        Ok(None)
    }
}

fn parse_timestamp(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
