use agent_settings::AgentSettings;
use anyhow::{Context, Result};
use gpui::App;
use indoc::indoc;
use settings::Settings as _;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use util::path_list::PathList;
use uuid::Uuid;

/// Fallback PostgreSQL connection URL when `ZED_THREADS_DATABASE_URL` is unset.
pub const DEFAULT_THREADS_DATABASE_URL: &str = "postgres://localhost/zed_threads";

/// The PostgreSQL connection URL for agent thread storage, resolved from the
/// `agent.threads_database_url` setting, then `ZED_THREADS_DATABASE_URL`, then a
/// localhost default.
pub fn threads_database_url(cx: &App) -> String {
    AgentSettings::try_get(cx)
        .and_then(|settings| settings.threads_database_url.clone())
        .or_else(|| zed_env_vars::ZED_THREADS_DATABASE_URL.clone())
        .unwrap_or_else(|| DEFAULT_THREADS_DATABASE_URL.to_string())
}

const SCHEMA: &[&str] = &[
    indoc!(
        "CREATE TABLE IF NOT EXISTS workspaces (
             workspace_id UUID PRIMARY KEY,
             name         TEXT NOT NULL,
             created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
             updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
         )"
    ),
    indoc!(
        "CREATE TABLE IF NOT EXISTS workspace_projects (
             workspace_id UUID NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
             path         TEXT NOT NULL,
             position     INT  NOT NULL DEFAULT 0,
             remote_connection_id BIGINT,
             PRIMARY KEY (workspace_id, path)
         )"
    ),
];

/// PostgreSQL-backed store for the named-workspace entity that threads are
/// grouped under.
///
/// This is the source of truth for *which workspace a thread belongs to*. It is
/// deliberately distinct from Zed's SQLite `WorkspaceDb`, whose
/// `workspace::WorkspaceId(i64)` is a per-machine layout id. The two identities
/// are bridged at the runtime boundary rather than sharing storage, so the
/// SQLite layout database (panes/breakpoints/bookmarks/docks) stays untouched.
#[derive(Clone)]
pub struct WorkspaceStore {
    pool: PgPool,
}

impl WorkspaceStore {
    pub async fn connect_with_url(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await
            .with_context(|| format!("failed to connect to thread database at {url}"))?;
        Self::ensure_schema(&pool).await?;
        Ok(Self { pool })
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn ensure_schema(pool: &PgPool) -> Result<()> {
        for statement in SCHEMA {
            sqlx::query(statement)
                .execute(pool)
                .await
                .context("failed to initialize thread database schema")?;
        }
        Ok(())
    }

    /// Return the id of the workspace whose member projects are exactly
    /// `project_paths`, creating it (with a derived placeholder name) if none
    /// exists. The returned id is stable across instances: it is looked up by
    /// the member set, never derived from a per-machine integer.
    pub async fn resolve_workspace(&self, project_paths: &PathList) -> Result<Uuid> {
        let paths = project_paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        if paths.is_empty() {
            anyhow::bail!("cannot resolve a workspace for an empty project set");
        }

        if let Some(id) = self.workspace_id_for_paths(&paths).await? {
            return Ok(id);
        }

        self.create_workspace(&paths).await
    }

    async fn workspace_id_for_paths(&self, paths: &[String]) -> Result<Option<Uuid>> {
        let id = sqlx::query_scalar::<_, Uuid>(
            "SELECT p.workspace_id
             FROM workspace_projects p
             GROUP BY p.workspace_id
             HAVING COUNT(*) = $1
                AND COUNT(*) FILTER (WHERE p.path = ANY($2::text[])) = $1",
        )
        .bind(paths.len() as i64)
        .bind(paths.to_vec())
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    async fn create_workspace(&self, paths: &[String]) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let name = derive_workspace_name(paths);

        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO workspaces (workspace_id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(&name)
            .execute(&mut *transaction)
            .await?;

        for (position, path) in paths.iter().enumerate() {
            sqlx::query(
                "INSERT INTO workspace_projects (workspace_id, path, position)
                 VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(path)
            .bind(position as i32)
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        Ok(id)
    }
}

/// Placeholder workspace name until the Layer 2 naming UI lands. Mirrors Zed's
/// current comma-joined project display, using only the last path component.
fn derive_workspace_name(paths: &[String]) -> String {
    let names = paths
        .iter()
        .map(|path| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone())
        })
        .collect::<Vec<_>>();
    names.join(", ")
}
